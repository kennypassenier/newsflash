//! The one loop (AR13): poll → settle → repeat, with the AR9 error
//! classes, the AR22 daemon hold, AR7 policy assertion on connect, and
//! M4 graceful shutdown.

use crate::config::Config;
use crate::hub_client::{HubClient, PolicyOutcome};
use crate::render::{daemon_present, show_toast_interactive, watch_interactive_toast};
use crate::{logx, state};
use courier_core::action_result::{ACTIONS_TOPIC, build_action_result};
use courier_core::backoff::retry_delay_secs;
use courier_core::envelope::parse_from_hub;
use courier_core::hub::{HubErrorClass, classify_receive_status, is_stale};
use courier_core::settle::{PreRender, SettleCallOutcome, pre_render};
use courier_core::toast::{actions_are_truncated, interactive_wait_cap_ms, toast_spec};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// M10: a fresh id for the action_result envelope newsflash publishes
/// on a click — same shape as send_test's, this side just needs
/// something unique, not globally meaningful.
fn fresh_action_result_id() -> String {
    let millis = now_ms();
    format!("action-{millis}-{}", std::process::id())
}

/// Sleep in small slices so a shutdown signal is honored promptly even
/// mid-backoff (M4).
fn sleep_interruptible(secs: u64, term: &AtomicBool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline && !term.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn run(config: Config) -> i32 {
    let term = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        // Conditional shutdown FIRST: the second signal aborts at once
        // (AR14); the first only sets the flag.
        let _ = signal_hook::flag::register_conditional_shutdown(sig, 130, Arc::clone(&term));
        let _ = signal_hook::flag::register(sig, Arc::clone(&term));
    }

    logx::info(&format!(
        "newsflash {} starting: hub={} topic={} subscription={} language={:?} ttl={}min sound={}",
        env!("CARGO_PKG_VERSION"),
        config.hub_url,
        config.topic,
        config.subscription,
        config.language,
        config.ttl_ms / 60_000,
        config
            .sound_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "off".into()),
    ));

    let client = HubClient::new(&config);
    let seen_path = state::state_path();
    let mut seen = state::load(&seen_path);

    let mut attempt: u32 = 0; // consecutive failed cycles (AR9)
    let mut connected = false; // for the recovered transition
    let mut policy_asserted = false; // re-asserted on every reconnect (AR7)
    let mut polled_once = false; // AR7: the first poll creates the subscription
    let mut from_beginning = true; // AR7 cold start: replay retained on the first poll
    let mut daemon_ok = false; // AR22 hold state
    let mut last_state = String::new(); // log only state CHANGES (AR9)

    while !term.load(Ordering::Relaxed) {
        // AR22: hold while the notification daemon is absent — messages
        // wait at the hub under the TTL, which is the designed behaviour.
        if !daemon_ok {
            daemon_ok = daemon_present();
            if !daemon_ok {
                log_state_change(
                    &mut last_state,
                    "no notification daemon on the session bus — holding, not consuming",
                );
                attempt += 1;
                sleep_interruptible(retry_delay_secs(attempt), &term);
                continue;
            }
            log_state_change(&mut last_state, "notification daemon present");
            attempt = 0;
        }

        // AR7: policy after the first successful poll, and again after
        // every reconnect.
        if polled_once && !policy_asserted {
            match client.ensure_policy(config.ttl_ms) {
                Ok(PolicyOutcome::AlreadyRight) => policy_asserted = true,
                Ok(PolicyOutcome::Written { overrode_explicit }) => {
                    policy_asserted = true;
                    if overrode_explicit {
                        logx::warn(
                            "policy asserted: ttl_ms written and an explicit dashboard edit was \
                             overridden — the policy is code (AR7); edit config.toml instead",
                        );
                    } else {
                        logx::info("policy asserted: ttl_ms written");
                    }
                }
                Err(e) => {
                    // Transient by decision (AR7): never a startup exit.
                    logx::warn(&format!(
                        "policy assert failed ({}); will retry — messages meanwhile follow \
                         the hub's current policy",
                        e.detail
                    ));
                }
            }
        }

        match client.receive(from_beginning) {
            Ok(none_or_message) => {
                if !connected {
                    log_state_change(&mut last_state, "hub reachable");
                    connected = true;
                    policy_asserted = false; // AR7: re-assert on reconnect
                    attempt = 0;
                }
                if !polled_once {
                    polled_once = true;
                }
                from_beginning = false;
                let Some((message, notice)) = none_or_message else {
                    continue; // 204 — the normal state of a healthy queue
                };
                if let Some(notice) = notice {
                    logx::info(&format!("hub notice: {notice}"));
                }
                if handle_message(&config, &client, &mut seen, &seen_path, &message) {
                    // AR22: a render failure may mean the daemon left the
                    // bus (logout race, crash) — re-probe before consuming
                    // more; if it is present the probe passes at once.
                    daemon_ok = false;
                }
            }
            Err(e) => {
                connected = false;
                attempt += 1;
                let class = classify_receive_status(e.status);
                match class {
                    HubErrorClass::Unreachable => log_state_change(
                        &mut last_state,
                        &format!("hub unreachable ({}) — backing off", e.detail),
                    ),
                    HubErrorClass::Auth => log_state_change(
                        &mut last_state,
                        "hub rejected the token (401/403). Remedy: re-mint the newsflash \
                         app token on the hub's /apps page, update latch, restart this unit. \
                         Retrying meanwhile",
                    ),
                    HubErrorClass::Archived => {
                        // AR21: revive it ourselves, loudly.
                        logx::warn(
                            "subscription was archived after long inactivity; unarchiving it — \
                             the lapsed backlog is disposable by design (10-minute TTL)",
                        );
                        match client.unarchive() {
                            Ok(()) => {
                                logx::info("subscription unarchived; resuming");
                                // Pick up what the topic still retains;
                                // staleness acks the old, dedup the seen.
                                from_beginning = true;
                                attempt = 0;
                                continue;
                            }
                            Err(ue) => logx::error(&format!(
                                "unarchive failed ({}); will retry",
                                ue.detail
                            )),
                        }
                    }
                    HubErrorClass::TopicMissing => log_state_change(
                        &mut last_state,
                        "topic does not exist yet (nothing has ever published) — waiting; \
                         newsflash send-test creates it",
                    ),
                    HubErrorClass::Other => log_state_change(
                        &mut last_state,
                        &format!(
                            "hub answered {} ({}) — a client-side problem; backing off",
                            e.status.unwrap_or(0),
                            e.detail
                        ),
                    ),
                }
                sleep_interruptible(retry_delay_secs(attempt), &term);
            }
        }
    }

    state::save(&seen_path, &seen);
    logx::info("shutdown: in-flight work settled, dedup store persisted");
    0
}

/// Returns true when the render itself failed (AR22 re-probe signal).
fn handle_message(
    config: &Config,
    client: &HubClient,
    seen: &mut courier_core::dedup::SeenSet,
    seen_path: &std::path::PathBuf,
    message: &courier_core::hub::HubMessage,
) -> bool {
    let parsed = parse_from_hub(&message.payload);
    let stale = is_stale(message.published_at_ms, config.ttl_ms, now_ms());
    match pre_render(&parsed, &message.id, seen, stale) {
        PreRender::Render => {
            let env = parsed.as_ref().expect("Render implies parsed");
            if let Some(p) = env.priority.as_deref()
                && !["info", "warning", "critical"].contains(&p)
            {
                // M11: the tolerant mapping must not be silent — when
                // pipeline-v2 grows a new priority, the journal says so.
                logx::warn(&format!(
                    "{}: unknown priority {p:?} rendered as info (AR4 tolerant reader)",
                    message.id
                ));
            }
            if actions_are_truncated(env) {
                logx::warn(&format!(
                    "{}: more than 2 actions on the envelope — only the first 2 are shown (M10)",
                    message.id
                ));
            }
            let spec = toast_spec(env, config.language);

            // M10 (AR13 amendment): interactive toasts block on the
            // user's answer, so "delivered" (ack, within the hub's
            // lease) and "answered" (may take arbitrarily long, or
            // never, for critical) are decoupled — the spawn/grace
            // check settles the message here; the click, if any, is
            // handled on a detached watcher below, exactly like the
            // existing sound thread.
            match show_toast_interactive(&spec) {
                Ok(child) => {
                    seen.insert(&message.id);
                    state::save(seen_path, seen);
                    settle_logged(client, &message.id, true, false);
                    logx::info(&format!(
                        "rendered {} (payload id {}, attempt {})",
                        message.id, env.id, message.attempt
                    ));
                    if let Some(sound) = &config.sound_file {
                        crate::render::play_sound(sound);
                    }

                    let max_wait =
                        interactive_wait_cap_ms(spec.expire_ms, config.interactive_wait_margin_ms)
                            .map(|ms| Duration::from_millis(ms as u64));
                    let payload_id = env.id.clone();
                    let ack_id = env.ack_id.clone();
                    let hub_id = message.id.clone();
                    let watcher_client = client.clone();
                    watch_interactive_toast(child, max_wait, move |action| {
                        let Some(action_id) = action else {
                            logx::info(&format!(
                                "{hub_id}: toast dismissed or timed out, no action chosen"
                            ));
                            return;
                        };
                        logx::info(&format!("{hub_id}: action {action_id:?} chosen"));
                        let body = build_action_result(
                            &fresh_action_result_id(),
                            &payload_id,
                            ack_id.as_deref(),
                            &action_id,
                        );
                        match watcher_client.publish_to(ACTIONS_TOPIC, &body) {
                            Ok(published_id) => logx::info(&format!(
                                "{hub_id}: action_result {published_id} published to {ACTIONS_TOPIC}"
                            )),
                            Err(e) => logx::warn(&format!(
                                "{hub_id}: failed to publish action_result ({}): {}",
                                e.status
                                    .map(|s| s.to_string())
                                    .unwrap_or("transport".into()),
                                e.detail
                            )),
                        }
                    });
                    false
                }
                Err(reason) => {
                    logx::warn(&format!(
                        "render failed for {} ({reason}) — nacked for redelivery; re-probing the daemon",
                        message.id
                    ));
                    settle_logged(client, &message.id, false, false);
                    true
                }
            }
        }
        PreRender::AckSilently => {
            settle_logged(client, &message.id, true, false);
            logx::info(&format!(
                "{}: redelivery of a seen id — acked silently",
                message.id
            ));
            false
        }
        PreRender::AckStale => {
            settle_logged(client, &message.id, true, false);
            logx::info(&format!(
                "{}: past the {}min TTL client-side (claimed before a suspend?) — acked unrendered",
                message.id,
                config.ttl_ms / 60_000
            ));
            false
        }
        PreRender::Poison(err) => {
            logx::warn(&format!(
                "{}: poison ({err:?}) — dead-lettered. Remedy: {}",
                message.id,
                err.remedy()
            ));
            settle_logged(client, &message.id, false, true);
            false
        }
    }
}

/// AR5's settle-call row: one bounded retry on transport trouble, then
/// let lease expiry redeliver (dedup absorbs it). Never a retry loop.
fn settle_logged(client: &HubClient, id: &str, ack: bool, dead: bool) {
    for round in 0..2 {
        let (outcome, detail) = client.settle(id, ack, dead);
        match outcome {
            SettleCallOutcome::Settled => return,
            SettleCallOutcome::GoneAnyway => {
                logx::info(&format!(
                    "settle of {id} answered a 4xx ({detail}) — lease or message already gone; \
                     treating as settled"
                ));
                return;
            }
            SettleCallOutcome::Retry if round == 0 => {
                std::thread::sleep(Duration::from_secs(1));
            }
            SettleCallOutcome::Retry => {
                logx::warn(&format!(
                    "settle of {id} kept failing ({detail}) — leaving it to lease expiry; \
                     dedup will absorb the redelivery"
                ));
            }
        }
    }
}

fn log_state_change(last: &mut String, state: &str) {
    if last != state {
        logx::info(state);
        *last = state.to_string();
    }
}
