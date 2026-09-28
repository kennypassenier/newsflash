//! feat-10: while a fullscreen app (a game, a presentation) is in front,
//! toasts go to Notification Center silently, and pop up once it is gone
//! (Kenny, 2026-09-28, after a toast crossed Oblivion Remastered).
//!
//! Detection is the shell's own answer to "may I notify the user now?"
//! (`SHQueryUserNotificationState`), the same signal Windows' automatic
//! "when playing a game" rule is built on — but that rule only covers
//! exclusive fullscreen, and a borderless-windowed game reports `BUSY`.

use crate::toast::{notifier, show_built};
use courier_core::wintoast::WinToast;
use newsflash::logx;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};

/// How often the waiting thread asks whether the fullscreen app is gone.
const CHECK_EVERY: Duration = Duration::from_secs(2);

pub fn app_in_front() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => [
            QUNS_BUSY,
            QUNS_RUNNING_D3D_FULL_SCREEN,
            QUNS_PRESENTATION_MODE,
        ]
        .contains(&state),
        // Unknown means we cannot tell: show normally rather than hide.
        Err(_) => false,
    }
}

/// One toast waiting for its popup: the version to show once the
/// fullscreen app is gone (same tag as the silent copy it replaces).
pub struct Held {
    pub toast: WinToast,
    pub label: String,
}

#[derive(Clone, Default)]
pub struct HeldQueue(Arc<Mutex<Vec<Held>>>);

impl HeldQueue {
    pub fn push(&self, held: Held) {
        if let Ok(mut q) = self.0.lock() {
            q.push(held);
        }
    }

    /// Starts the thread that pops the held toasts up after the
    /// fullscreen app is gone, then plays the chime once for the batch.
    pub fn start(&self, chime: Option<PathBuf>) {
        let queue = self.clone();
        let _ = std::thread::Builder::new()
            .name("fullscreen-release".into())
            .spawn(move || {
                // WinRT needs COM on this thread too.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut live = VecDeque::new();
                loop {
                    std::thread::sleep(CHECK_EVERY);
                    let waiting = queue.0.lock().map(|q| !q.is_empty()).unwrap_or(false);
                    if !waiting || app_in_front() {
                        continue;
                    }
                    let batch: Vec<Held> = match queue.0.lock() {
                        Ok(mut q) => q.drain(..).collect(),
                        Err(_) => continue,
                    };
                    release(batch, chime.as_deref(), &mut live);
                }
            });
    }
}

fn release(
    batch: Vec<Held>,
    chime: Option<&Path>,
    live: &mut VecDeque<windows::UI::Notifications::ToastNotification>,
) {
    let Ok(notifier) = notifier() else { return };
    let now = newsflash::run::now_ms();
    let mut shown = 0;
    for held in batch {
        if held.toast.expires_at_ms.is_some_and(|at| at <= now) {
            logx::info(&format!(
                "{}: its lifetime ended during the fullscreen app — not popped up",
                held.label
            ));
            continue;
        }
        match show_built(&notifier, &held.toast, &held.label, live) {
            Ok(()) => {
                logx::info(&format!(
                    "{}: fullscreen app closed — popped up now",
                    held.label
                ));
                shown += 1;
            }
            Err(e) => logx::warn(&format!(
                "{}: popping up after fullscreen failed ({e}) — it stays in Notification Center",
                held.label
            )),
        }
    }
    if shown > 0
        && let Some(chime) = chime
    {
        crate::toast::play_chime(chime);
    }
}
