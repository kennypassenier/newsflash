//! Install / uninstall on Windows, shared by `newsflash install` (CLI)
//! and the setup wizard. Output goes through `log` so each caller shows
//! it its own way (console lines vs the wizard's log pane).

use crate::{instance, paths, registry};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Options {
    pub add_to_path: bool,
    pub autostart: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            add_to_path: true,
            autostart: true,
        }
    }
}

/// What happened, so the CLI can tell the next step.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    /// Config was missing — a starter one was written; nothing started.
    NeedsConfig,
    /// Config present but unusable (the remedy).
    ConfigUnusable(String),
    Started,
}

/// Built names (cargo) or installed names (re-running from the install
/// directory is a harmless re-register).
fn sources() -> Result<(PathBuf, PathBuf), String> {
    let current = std::env::current_exe()
        .map_err(|e| format!("cannot determine where newsflash lives: {e}"))?;
    let dir = current.parent().map(Path::to_path_buf).unwrap_or_default();
    let (console, windowless) = if dir.join("newsflash-winw.exe").is_file() {
        (
            dir.join("newsflash-win.exe"),
            dir.join("newsflash-winw.exe"),
        )
    } else {
        (dir.join("newsflash.exe"), dir.join("newsflashw.exe"))
    };
    for f in [&console, &windowless] {
        if !f.is_file() {
            return Err(format!(
                "{} is missing — newsflash.exe and newsflashw.exe must sit side by side.",
                f.display()
            ));
        }
    }
    Ok((console, windowless))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The daemon lets go of its mutex a moment before its process (and the
/// lock Windows keeps on a running .exe) is gone — seen live 2026-09-24,
/// when the copy lost that race. Retry for up to 10 s instead of failing
/// halfway with the old daemon already stopped.
fn copy_when_released(from: &Path, to: &Path) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match std::fs::copy(from, to) {
            Ok(_) => return Ok(()),
            // ERROR_SHARING_VIOLATION: the old process still holds it.
            Err(e) if e.raw_os_error() == Some(32) && std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return Err(format!(
                    "copying {} → {}: {e}",
                    from.display(),
                    to.display()
                ));
            }
        }
    }
}

pub fn stop_running(log: &mut dyn FnMut(String)) -> Result<(), String> {
    if instance::is_running() {
        log("stopping the running newsflash…".into());
        instance::request_stop();
        if !instance::wait_stopped(Duration::from_secs(45)) {
            return Err(
                "the running newsflash did not stop within 45 s — try `newsflash stop`.".into(),
            );
        }
    }
    Ok(())
}

pub fn install(options: &Options, log: &mut dyn FnMut(String)) -> Result<Installed, String> {
    let (console_src, windowless_src) = sources()?;
    stop_running(log)?;

    let dest = paths::install_dir();
    std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let console_dst = dest.join("newsflash.exe");
    let windowless_dst = dest.join("newsflashw.exe");
    for (from, to) in [
        (&console_src, &console_dst),
        (&windowless_src, &windowless_dst),
    ] {
        if !same_file(from, to) {
            copy_when_released(from, to)?;
        }
    }
    log(format!("installed to {}", dest.display()));

    crate::app::ensure_assets();
    registry::register(&windowless_dst, &paths::assets_dir().join("app.png"))
        .map_err(|e| format!("registering with Windows failed: {e}"))?;
    registry::set_autostart(options.autostart.then_some(windowless_dst.as_path()))?;
    registry::register_uninstall_entry(&windowless_dst, &dest, env!("CARGO_PKG_VERSION"))?;
    log(format!(
        "registered: toast identity, click activator{}, Installed apps entry",
        if options.autostart {
            ", start at logon"
        } else {
            ""
        }
    ));
    match crate::shortcut::create(&windowless_dst) {
        Ok(lnk) => log(format!("Start menu shortcut: {}", lnk.display())),
        Err(e) => log(format!("no Start menu shortcut ({e}) — not needed to run")),
    }

    if options.add_to_path {
        match registry::add_to_user_path(&dest) {
            Ok(true) => log(format!(
                "added {} to your PATH — open a NEW terminal to type just `newsflash`",
                dest.display()
            )),
            Ok(false) => {}
            Err(e) => log(format!(
                "could not add {} to your PATH: {e}",
                dest.display()
            )),
        }
    } else if let Ok(true) = registry::remove_from_user_path(&dest) {
        log("removed newsflash from your PATH (not wanted any more)".into());
    }

    let config_path = paths::config_path();
    if !config_path.is_file() {
        let _ = std::fs::create_dir_all(paths::config_dir());
        std::fs::write(&config_path, crate::app::CONFIG_EXAMPLE)
            .map_err(|e| format!("cannot write {}: {e}", config_path.display()))?;
        log(format!(
            "wrote a starter config to {}",
            config_path.display()
        ));
        return Ok(Installed::NeedsConfig);
    }
    match newsflash::chime::ensure(&paths::data_dir(), &config_path)? {
        newsflash::chime::Outcome::Configured(wav) => {
            log(format!("chime: sound_file now points at {}", wav.display()))
        }
        newsflash::chime::Outcome::KeptExisting(s) => {
            log(format!("chime: keeping the sound_file you set ({s})"))
        }
    }
    if let Err(remedy) = crate::app::load_config() {
        return Ok(Installed::ConfigUnusable(remedy));
    }
    crate::app::start_detached(&windowless_dst)
        .map_err(|e| format!("could not start {}: {e}", windowless_dst.display()))?;
    log(if options.autostart {
        "started. It will also start by itself at every logon.".into()
    } else {
        "started (not at logon, by your choice).".into()
    });
    Ok(Installed::Started)
}

pub fn uninstall(log: &mut dyn FnMut(String)) -> Result<(), String> {
    if instance::is_running() {
        log("stopping newsflash…".into());
        instance::request_stop();
        instance::wait_stopped(Duration::from_secs(45));
    }
    registry::unregister()?;
    registry::remove_uninstall_entry()?;
    log(
        "unregistered: toast identity, click activator, start at logon, Installed apps entry"
            .into(),
    );
    if crate::shortcut::remove() {
        log("removed the Start menu shortcut".into());
    }
    match registry::remove_from_user_path(&paths::install_dir()) {
        Ok(true) => log("removed newsflash from your PATH".into()),
        Ok(false) => {}
        Err(e) => log(format!("could not remove newsflash from your PATH: {e}")),
    }
    if let Ok(history) = windows::UI::Notifications::ToastNotificationManager::History() {
        let _ = history.ClearWithId(&windows::core::HSTRING::from(crate::AUMID));
    }
    log(format!(
        "left in place (delete by hand if wanted):\n  {}\n  {}\n  {}",
        paths::install_dir().display(),
        paths::config_dir().display(),
        paths::data_dir().display()
    ));
    Ok(())
}
