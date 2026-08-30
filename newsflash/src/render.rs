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

use courier_core::toast::ToastSpec;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

fn build_toast_command(spec: &ToastSpec) -> Command {
    let mut cmd = Command::new("notify-send");
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
    let mut cmd = build_toast_command(spec);
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
    mut child: Child,
    max_wait: Option<Duration>,
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
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut out);
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
