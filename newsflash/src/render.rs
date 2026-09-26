//! The desktop side of the shell (K2, K6, AR12, AR19, AR22):
//! subprocesses resolved via PATH so tests shim them, every child
//! under a hard timeout so a D-Bus stall can never freeze the loop.
//!
//! M10 (2026-08-30): every toast now carries action buttons, which
//! makes `notify-send` block until the user answers or the toast's own
//! expire timeout fires (`--action` implies `--wait`). Blocking the
//! main poll loop on that would freeze message consumption — forever,
//! for a critical toast (`expire_ms == 0`, measured empirically). So
//! rendering an interactive toast splits in two: `show_toast_interactive`
//! spawns it and confirms it did not fail within a short grace period
//! (AR13's philosophy stays: the settle table only needs to know
//! "delivered", i.e. shown, not "answered"), then the caller detaches
//! the still-running child to `watch_interactive_toast`, which behaves
//! like the existing sound thread: fire-and-forget from the loop's
//! perspective, its own failures only ever logged.

use crate::config::Config;
use crate::hub_client::HubClient;
use crate::logx;
use crate::run::{AfterAck, Desktop, publish_action_result};
use courier_core::envelope::Envelope;
use courier_core::hub::HubMessage;
use courier_core::toast::{
    Language, Lifetimes, ToastSpec, actions_are_truncated, interactive_wait_cap_ms,
    lifetime_minutes, resolve_link, toast_spec,
};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The Linux half of `run::Desktop`: notify-send toasts (K2/M10), the
/// busctl daemon probe (AR22) and the paplay chime (K6).
pub struct LinuxDesktop {
    client: HubClient,
    language: Language,
    sound_file: Option<PathBuf>,
    interactive_wait_margin_ms: u32,
    lifetimes: Lifetimes,
    link_base_url: Option<String>,
}

/// The freedesktop action a server fires for a click on the body itself
/// (Plasma draws no button for it).
const DEFAULT_ACTION: &str = "default";

impl LinuxDesktop {
    pub fn new(config: &Config) -> Self {
        LinuxDesktop {
            client: HubClient::new(config),
            language: config.language,
            sound_file: config.sound_file.clone(),
            interactive_wait_margin_ms: config.interactive_wait_margin_ms,
            lifetimes: config.lifetimes,
            link_base_url: config.link_base_url.clone(),
        }
    }
}

impl Desktop for LinuxDesktop {
    fn ready(&mut self) -> bool {
        daemon_present()
    }

    fn hold_reason(&self) -> String {
        "no notification daemon on the session bus — holding, not consuming".into()
    }

    fn ready_line(&self) -> &'static str {
        "notification daemon present"
    }

    fn show(&mut self, message: &HubMessage, env: &Envelope) -> Result<AfterAck, String> {
        if actions_are_truncated(env) {
            logx::warn(&format!(
                "{}: more than {} actions on the envelope — only the first {} are shown (M10)",
                message.id,
                courier_core::toast::MAX_ACTIONS,
                courier_core::toast::MAX_ACTIONS
            ));
        }
        let mut spec = toast_spec(env, self.language);
        // Clicking the notification itself opens click_url, when there is
        // one we may open (full http(s), or a path + link_base_url).
        let link = env
            .click_url
            .as_deref()
            .and_then(|u| resolve_link(u, self.link_base_url.as_deref()));
        if link.is_some() {
            spec.actions
                .push((DEFAULT_ACTION.to_string(), "Openen".to_string()));
        }
        // Link buttons (`url`, or the companion app's `"action": "URI"`)
        // open their page instead of replying to notify.actions.
        let link_buttons: Vec<(String, String)> = env
            .effective_actions()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                let url = resolve_link(a.url.as_deref()?, self.link_base_url.as_deref())?;
                Some((a.id, url))
            })
            .collect();
        // Ephemeral / per-priority lifetime: counted from publishing, so
        // time spent waiting at the hub counts too.
        let close_after = lifetime_minutes(env, &self.lifetimes).map(|m| {
            let end = message.published_at_ms + u64::from(m) * 60_000;
            Duration::from_millis(end.saturating_sub(crate::run::now_ms()).max(1_000))
        });
        let child = show_toast_with(&spec, close_after.is_some())?;

        let sound = self.sound_file.clone();
        let max_wait = interactive_wait_cap_ms(spec.expire_ms, self.interactive_wait_margin_ms)
            .map(|ms| Duration::from_millis(ms as u64));
        let payload_id = env.id.clone();
        let ack_id = env.ack_id.clone();
        let hub_id = message.id.clone();
        let watcher_client = self.client.clone();
        Ok(Box::new(move || {
            if let Some(sound) = &sound {
                play_sound(sound);
            }
            watch_toast(child, max_wait, close_after, move |action| {
                let Some(action_id) = action else {
                    logx::info(&format!(
                        "{hub_id}: toast dismissed or timed out, no action chosen"
                    ));
                    return;
                };
                if let Some((_, url)) = link_buttons.iter().find(|(id, _)| *id == action_id) {
                    logx::info(&format!("{hub_id}: opened {url}"));
                    let _ = Command::new("xdg-open").arg(url).spawn();
                    return;
                }
                if action_id == DEFAULT_ACTION {
                    if let Some(link) = &link {
                        logx::info(&format!("{hub_id}: opened {link}"));
                        let _ = Command::new("xdg-open").arg(link).spawn();
                    }
                    return;
                }
                logx::info(&format!("{hub_id}: action {action_id:?} chosen"));
                publish_action_result(
                    &watcher_client,
                    &hub_id,
                    &payload_id,
                    ack_id.as_deref(),
                    &action_id,
                    &[],
                );
            });
        }))
    }
}

pub const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

/// M10: how long `show_toast_interactive` waits before treating a
/// still-running notify-send as "legitimately showing, hand it off"
/// rather than "failed". Short — this only needs to catch instant
/// failures (bad argv, the daemon refusing outright), not the
/// interactive wait itself.
const SPAWN_GRACE: Duration = Duration::from_millis(300);

#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Ok,
    Failed(String),
}

/// Spawn + poll with a deadline; a child past the deadline is killed
/// and reported as failed (AR19 — settles as the transient row).
/// Public so tests can exercise the timeout branch with a short
/// deadline instead of waiting out CHILD_TIMEOUT.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> RunOutcome {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RunOutcome::Failed(format!("spawn failed: {e}")),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return RunOutcome::Ok,
            Ok(Some(status)) => return RunOutcome::Failed(format!("exit {status}")),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return RunOutcome::Failed(format!("timed out after {timeout:?}, killed"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return RunOutcome::Failed(format!("wait failed: {e}")),
        }
    }
}

fn build_toast_command(spec: &ToastSpec, print_id: bool) -> Command {
    let mut cmd = Command::new("notify-send");
    if print_id {
        // First stdout line = the notification id, needed to close it
        // when its lifetime ends (ephemeral / expire_*_minutes).
        cmd.arg("--print-id");
    }
    cmd.arg("--app-name=newsflash")
        .arg(format!("--urgency={}", spec.urgency.as_notify_send_arg()))
        .arg(format!("--expire-time={}", spec.expire_ms))
        .arg(format!("--icon={}", spec.icon));
    // -A before the -- separator like every other option; the id/label
    // pair itself is producer/default text and never treated as a flag
    // (AR12 hygiene applies to summary/body, not to notify-send's own
    // NAME=Text action syntax, which cannot be reinterpreted as another
    // option regardless of its content).
    for (id, label) in &spec.actions {
        cmd.arg("-A").arg(format!("{id}={label}"));
    }
    cmd.arg("--").arg(&spec.summary);
    if !spec.body.is_empty() {
        cmd.arg(&spec.body);
    }
    cmd
}

/// K2/M10: spawns the toast with its action buttons and confirms it
/// did not fail within `SPAWN_GRACE`. Does NOT wait for the user's
/// answer — see the module doc. `Ok(child)` hands back a still-running
/// (or, for a near-instant expiry, already-finished) process whose
/// stdout the caller reads later via `watch_interactive_toast`.
pub fn show_toast_interactive(spec: &ToastSpec) -> Result<Child, String> {
    show_toast_with(spec, false)
}

/// `show_toast_interactive`, optionally asking notify-send to print the
/// notification id first (for `watch_toast`'s `close_after`).
pub fn show_toast_with(spec: &ToastSpec, print_id: bool) -> Result<Child, String> {
    let mut cmd = build_toast_command(spec, print_id);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let deadline = Instant::now() + SPAWN_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(child),
            Ok(Some(status)) => return Err(format!("exit {status}")),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Ok(child); // still running = legitimately showing
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

/// M10: waits out an already-shown interactive toast on a detached
/// thread and reports which action (if any) was chosen. `max_wait`
/// bounds the wait only for toasts with their own expiry — pass `None`
/// for a persistent (critical) toast (see `interactive_wait_cap_ms` in
/// courier-core: capping it would kill the buttons long before Kenny
/// answers). `on_result` runs on the detached thread; it always fires,
/// with `None` for "no action chosen" (timeout, dismiss, or a wait/read
/// failure — each logged separately here, not distinguished for the
/// caller since none of them are the caller's problem to settle).
pub fn watch_interactive_toast(
    child: Child,
    max_wait: Option<Duration>,
    on_result: impl FnOnce(Option<String>) + Send + 'static,
) {
    watch_toast(child, max_wait, None, on_result);
}

/// Closes notification `id` on the session bus: gone from the screen
/// and from the server's history (the ephemeral promise).
pub fn close_notification(id: u32) -> RunOutcome {
    let mut cmd = Command::new("busctl");
    cmd.args([
        "--user",
        "--timeout=5",
        "call",
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "CloseNotification",
        "u",
    ])
    .arg(id.to_string());
    run_with_timeout(cmd, Duration::from_secs(6))
}

/// `watch_interactive_toast` plus a lifetime: with `close_after`, the
/// child must have been started with `print_id` — its first stdout line
/// is the id, and a timer closes that notification when the lifetime
/// ends (whether it is still on screen or already in history). The
/// click, if any, is read from the lines after the id.
pub fn watch_toast(
    mut child: Child,
    max_wait: Option<Duration>,
    close_after: Option<Duration>,
    on_result: impl FnOnce(Option<String>) + Send + 'static,
) {
    let Some(lifetime) = close_after else {
        return watch_plain(child, max_wait, on_result);
    };
    let Some(stdout) = child.stdout.take() else {
        return watch_plain(child, max_wait, on_result);
    };
    // Reads the id at once (to arm the timer), then whatever follows.
    let reader = std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        let id = lines
            .next()
            .and_then(Result::ok)
            .and_then(|l| l.trim().parse::<u32>().ok());
        match id {
            Some(id) => {
                std::thread::spawn(move || {
                    std::thread::sleep(lifetime);
                    if let RunOutcome::Failed(reason) = close_notification(id) {
                        crate::logx::warn(&format!(
                            "could not close expired notification {id} ({reason})"
                        ));
                    }
                });
            }
            None => crate::logx::warn(
                "notify-send printed no notification id — this toast cannot expire early",
            ),
        }
        lines.map_while(Result::ok).collect::<Vec<_>>().join("\n")
    });
    watch_with_reader(child, max_wait, Some(reader), on_result);
}

fn watch_plain(
    child: Child,
    max_wait: Option<Duration>,
    on_result: impl FnOnce(Option<String>) + Send + 'static,
) {
    watch_with_reader(child, max_wait, None, on_result);
}

fn watch_with_reader(
    mut child: Child,
    max_wait: Option<Duration>,
    reader: Option<std::thread::JoinHandle<String>>,
    on_result: impl FnOnce(Option<String>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let deadline = max_wait.map(|d| Instant::now() + d);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if deadline.is_some_and(|dl| Instant::now() >= dl) {
                        let _ = child.kill();
                        let _ = child.wait();
                        crate::logx::warn(
                            "interactive toast hit its safety-cap wait and was killed — \
                             the notification may still be visible but its buttons are \
                             now dead (the process that owned them exited)",
                        );
                        on_result(None);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    crate::logx::warn(&format!("interactive toast wait failed: {e}"));
                    on_result(None);
                    return;
                }
            }
        }
        let mut out = String::new();
        match reader {
            Some(r) => out = r.join().unwrap_or_default(),
            None => {
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
            }
        }
        let action = out.trim();
        on_result(if action.is_empty() {
            None
        } else {
            Some(action.to_string())
        });
    });
}

/// AR22: probe the notification daemon with a query (never a visible
/// notification). While this fails the loop holds instead of consuming.
pub fn daemon_present() -> bool {
    let mut cmd = Command::new("busctl");
    cmd.args([
        "--user",
        "--timeout=5",
        "call",
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "GetServerInformation",
    ]);
    run_with_timeout(cmd, Duration::from_secs(6)) == RunOutcome::Ok
}

/// K6: fire-and-forget chime on a detached thread; a player problem is
/// the caller's log line at most, never a failed message (AR11). At
/// SIGTERM the thread may die mid-chime — deliberate, do not join.
pub fn play_sound(file: &Path) {
    let file = file.to_path_buf();
    std::thread::spawn(move || {
        let mut cmd = Command::new("paplay");
        cmd.arg("--").arg(&file);
        if let RunOutcome::Failed(reason) = run_with_timeout(cmd, CHILD_TIMEOUT) {
            crate::logx::warn(&format!("chime failed ({reason}) — toast was shown anyway"));
        }
    });
}
