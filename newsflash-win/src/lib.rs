//! newsflash for Windows 11 — the same kyu subscription (`desktop` on
//! `notify.kenny`) rendered as native Windows toasts in Notification
//! Center, so Kenny gets his notifications whichever OS he boots
//! (docs/WINDOWS.md). Only one OS runs at a time, so there is only
//! ever one consumer on the subscription.
//!
//! Split like the Linux crate: the loop, hub client, config and dedup
//! store come from the `newsflash` library; every toast decision is in
//! `courier_core::wintoast`. This crate is the thin Windows shell.
//! Modules without `cfg(windows)` are plain std and tested on any OS.

pub mod images;
pub mod logfile;
pub mod paths;
pub mod winconfig;

#[cfg(windows)]
pub mod activator;
#[cfg(windows)]
pub mod app;
#[cfg(windows)]
mod demo;
#[cfg(windows)]
pub mod installer;
#[cfg(windows)]
pub mod instance;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod secret;
#[cfg(windows)]
mod setup;
#[cfg(windows)]
mod shortcut;
#[cfg(windows)]
pub mod toast;

/// The AppUserModelID toasts are shown under (registered by `install`).
pub const AUMID: &str = "Newsflash.Kyu";
