//! One daemon per logon session, and a clean way to stop it — the
//! systemd unit's job on Linux (AR20, M4). A named mutex marks "the
//! daemon is running"; a named event is the SIGTERM equivalent
//! (`newsflash stop`, and `install` before it replaces the binaries).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, OpenMutexW,
    SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
};
use windows::core::w;

const DAEMON_MUTEX: windows::core::PCWSTR = w!("Local\\newsflash-daemon");
const STOP_EVENT: windows::core::PCWSTR = w!("Local\\newsflash-stop");

/// Held for the daemon's lifetime; released (closed) on drop or exit.
pub struct DaemonLock(HANDLE);

impl Drop for DaemonLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// `None` = another daemon already runs in this session.
pub fn acquire() -> Option<DaemonLock> {
    unsafe {
        let handle = CreateMutexW(None, true, DAEMON_MUTEX).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(DaemonLock(handle))
    }
}

pub fn is_running() -> bool {
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, DAEMON_MUTEX) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// Creates the stop event and a watcher thread that raises `term`
/// when it is signalled. The thread ends with the process.
pub fn watch_stop_event(term: Arc<AtomicBool>) -> Result<(), String> {
    let handle = unsafe { CreateEventW(None, true, false, STOP_EVENT) }
        .map_err(|e| format!("cannot create the stop event: {}", e.message()))?;
    // HANDLE wraps a raw pointer (not Send); the kernel handle itself is
    // process-wide, so moving its value to the watcher is sound.
    let raw = handle.0 as usize;
    std::thread::spawn(move || {
        let handle = HANDLE(raw as *mut core::ffi::c_void);
        while !term.load(Ordering::Relaxed) {
            if unsafe { WaitForSingleObject(handle, 500) }.0 == 0 {
                crate::app::log_stop_request();
                term.store(true, Ordering::Relaxed);
            }
        }
    });
    Ok(())
}

/// Signals a running daemon to stop; `false` if none is listening.
pub fn request_stop() -> bool {
    unsafe {
        match OpenEventW(EVENT_MODIFY_STATE, false, STOP_EVENT) {
            Ok(h) => {
                let ok = SetEvent(h).is_ok();
                let _ = CloseHandle(h);
                ok
            }
            Err(_) => false,
        }
    }
}

/// Waits until the daemon has let go of its mutex. Bounded like the
/// unit's TimeoutStopSec: one poll read plus a settle (AR19).
pub fn wait_stopped(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !is_running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !is_running()
}
