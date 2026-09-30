//! The one loop (AR13): poll → settle → repeat, with the AR9 error
//! classes, the AR22 daemon hold, AR7 policy assertion on connect, and
//! M4 graceful shutdown.
//!
//! Windows port: the loop is shared by both desktops. Everything
//! platform-specific sits behind `Desktop` — Linux implements it in
//! `render::LinuxDesktop` (notify-send), newsflash-win with WinRT
//! toasts. The settle table, dedup, TTL, backoff and policy logic stay
//! in one place so both OSes consume the one `desktop` subscription
//! identically.

use crate::config::Config;
use crate::hub_client::{HubClient, PolicyOutcome};
use crate::render::LinuxDesktop;
use crate::{logx, state};
use courier_core::action_result::{ACTIONS_TOPIC, build_action_result};
use courier_core::backoff::retry_delay_secs;
use courier_core::envelope::{Envelope, parse_from_hub};
use courier_core::hub::{HubErrorClass, HubMessage, classify_receive_status, is_stale};
use courier_core::settle::{PreRender, SettleCallOutcome, pre_render};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Runs after the ack (AR24: settle never waits on the user) — the
/// chime, the click watcher. May be a no-op.
pub type AfterAck = Box<dyn FnOnce()>;

/// What the loop needs from a desktop.
pub trait Desktop {
    /// AR22: can a toast be shown right now? `false` = hold, don't
    /// consume — messages wait at the hub under the TTL.
    fn ready(&mut self) -> bool;
    /// State line logged (once) while `ready` is false.
    fn hold_reason(&self) -> String;
    /// State line logged (once) when `ready` turns true.
    fn ready_line(&self) -> &'static str;
    /// K2: show one parse-validated, fresh envelope. `Ok` = shown; the
    /// loop marks it seen, acks, logs, then runs the returned hook.
    /// `Err` = transient render failure: nack + re-probe (K3, AR22).
    fn show(&mut self, message: &HubMessage, env: &Envelope) -> Result<AfterAck, String>;
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// M10: a fresh id for the action_result envelope newsflash publishes
/// on a click — same shape as send_test's, this side just needs
/// something unique, not globally meaningful.
pub fn fresh_action_result_id() -> String {
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

/// The Linux entry point: signals, the XDG state path, notify-send.
pub fn run(config: Config) -> i32 {
    let term = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        // Conditional shutdown FIRST: the second signal aborts at once
        // (AR14); the first only sets the flag.
        let _ = signal_hook::flag::register_conditional_shutdown(sig, 130, Arc::clone(&term));
        let _ = signal_hook::flag::register(sig, Arc::clone(&term));
    }
    let mut desktop = LinuxDesktop::new(&config);
    run_with(&config, &mut desktop, &state::state_path(), &term)
}

/// The shared loop. `term` is the caller's shutdown flag (signals on
/// Linux, a named event / Ctrl+C on Windows).
pub fn run_with(
    config: &Config,
    desktop: &mut dyn Desktop,
    seen_path: &Path,
    term: &AtomicBool,
) -> i32 {
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

    let client = HubClient::new(config);
    let seen_path = seen_path.to_path_buf();
    let mut seen = state::load(&seen_path);
    crate::snooze::set_minutes(config.snooze_minutes);

    let mut attempt: u32 = 0; // consecutive failed cycles (AR9)
    let mut connected = false; // for the recovered transition
    let mut policy_asserted = false; // re-asserted on every reconnect (AR7)
    let mut polled_once = false; // AR7: the first poll creates the subscription
    let mut from_beginning = true; // AR7 cold start: replay retained on the first poll
    let mut daemon_ok = false; // AR22 hold state
    let mut last_state = String::new(); // log only state CHANGES (AR9)

    while !term.load(Ordering::Relaxed) {
        if daemon_ok {
            show_snoozed(config, desktop);
        }
        // AR22: hold while the notification daemon is absent — messages
        // wait at the hub under the TTL, which is the designed behaviour.
        if !daemon_ok {
            daemon_ok = desktop.ready();
            if !daemon_ok {
                log_state_change(&mut last_state, &desktop.hold_reason());
                attempt += 1;
                sleep_interruptible(retry_delay_secs(attempt), term);
                continue;
            }
            log_state_change(&mut last_state, desktop.ready_line());
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
                if handle_message(config, &client, desktop, &mut seen, &seen_path, &message) {
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
                sleep_interruptible(retry_delay_secs(attempt), term);
            }
        }
    }

    state::save(&seen_path, &seen);
    logx::info("shutdown: in-flight work settled, dedup store persisted");
    0
}

/// feat-11: show again what was snoozed and is now due. Already acked,
/// so this never settles; a message whose lifetime ended meanwhile is
/// dropped instead of reappearing for a moment.
fn show_snoozed(config: &Config, desktop: &mut dyn Desktop) {
    for (message, env) in crate::snooze::take_due(now_ms()) {
        let ended = courier_core::toast::lifetime_minutes(&env, &config.lifetimes)
            .is_some_and(|m| message.published_at_ms + u64::from(m) * 60_000 <= now_ms());
        if ended {
            logx::info(&format!(
                "{}: snooze over, but its lifetime ended meanwhile — not shown again",
                message.id
            ));
            continue;
        }
        match desktop.show(&message, &env) {
            Ok(after) => {
                logx::info(&format!("{}: snooze over — shown again", message.id));
                crate::snooze::remember(&message, &env);
                after();
            }
            Err(e) => logx::warn(&format!(
                "{}: snooze over, but showing it again failed ({e})",
                message.id
            )),
        }
    }
}

/// Returns true when the render itself failed (AR22 re-probe signal).
fn handle_message(
    config: &Config,
    client: &HubClient,
    desktop: &mut dyn Desktop,
    seen: &mut courier_core::dedup::SeenSet,
    seen_path: &PathBuf,
    message: &HubMessage,
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

            // M10 (AR13 amendment): interactive toasts may wait on the
            // user's answer, so "delivered" (ack, within the hub's
            // lease) and "answered" (may take arbitrarily long, or
            // never, for critical) are decoupled — a successful show
            // settles the message here; the click, if any, is handled
            // by the desktop's own machinery after the ack.
            match desktop.show(message, env) {
                Ok(after_ack) => {
                    seen.insert(&message.id);
                    state::save(seen_path, seen);
                    settle_logged(client, &message.id, true, false);
                    logx::info(&format!(
                        "rendered {} (payload id {}, attempt {})",
                        message.id, env.id, message.attempt
                    ));
                    crate::snooze::remember(message, env);
                    after_ack();
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

/// M10 reply path, shared by both desktops: publish one action_result
/// to `notify.actions`. Failures are logged, never retried — the click
/// is a user gesture, not a queued obligation.
pub fn publish_action_result(
    client: &HubClient,
    hub_id: &str,
    payload_id: &str,
    ack_id: Option<&str>,
    action_id: &str,
    inputs: &[(String, String)],
) {
    let body = build_action_result(
        &fresh_action_result_id(),
        payload_id,
        ack_id,
        action_id,
        inputs,
    );
    match client.publish_to(ACTIONS_TOPIC, &body) {
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
