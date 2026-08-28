//! The desktop side of the shell (K2, K6, AR12, AR19, AR22):
//! subprocesses resolved via PATH so tests shim them, every child
//! under a hard timeout so a D-Bus stall can never freeze the loop.

use courier_core::toast::ToastSpec;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

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

/// K2: one toast. `--` before positionals: body content is data, never
/// options (AR12).
pub fn show_toast(spec: &ToastSpec) -> RunOutcome {
    let mut cmd = Command::new("notify-send");
    cmd.arg("--app-name=desk-courier")
        .arg(format!("--urgency={}", spec.urgency.as_notify_send_arg()))
        .arg(format!("--expire-time={}", spec.expire_ms))
        .arg("--")
        .arg(&spec.summary);
    if !spec.body.is_empty() {
        cmd.arg(&spec.body);
    }
    run_with_timeout(cmd, CHILD_TIMEOUT)
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
