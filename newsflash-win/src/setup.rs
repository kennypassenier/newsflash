//! `newsflash setup` on Windows: the wizard (newsflash-setup) over this
//! crate's installer, the DPAPI token store and the shared hub checks.
//! Also what a double-click on `newsflashw.exe` opens while newsflash is
//! not installed yet, and what Settings → Installed apps → Modify /
//! Uninstall open.

use crate::{app, installer, instance, paths, registry, secret};
use newsflash::config_edit;
use newsflash::hub_client::HubClient;
use newsflash::send_test;
use newsflash_setup::{Backend, Check, Choices, Existing};
use std::sync::Arc;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::HSTRING;

struct WinBackend;

fn config_text() -> String {
    std::fs::read_to_string(paths::config_path()).unwrap_or_default()
}

/// The token newsflash would use: env, then DPAPI, then token_file.
fn stored_token() -> Option<(String, &'static str)> {
    if let Ok(t) = std::env::var("KYU_TOKEN")
        && !t.trim().is_empty()
    {
        return Some((t.trim().to_string(), "KYU_TOKEN environment variable"));
    }
    if let Ok(Some(t)) = secret::load(&paths::token_path()) {
        return Some((t, "encrypted for your Windows account (DPAPI)"));
    }
    let file = config_edit::get_string(&config_text(), "token_file")?;
    let t = std::fs::read_to_string(file).ok()?;
    Some((t.trim().to_string(), "token_file")).filter(|(t, _)| !t.is_empty())
}

impl Backend for WinBackend {
    fn os_name(&self) -> &'static str {
        "Windows"
    }

    fn has_critical_scenario(&self) -> bool {
        true
    }

    fn detect(&self) -> Existing {
        let text = config_text();
        let token = stored_token();
        Existing {
            installed: registry::is_registered(),
            running: instance::is_running(),
            hub_url: config_edit::get_string(&text, "hub_url"),
            language: config_edit::get_string(&text, "language"),
            critical_scenario: config_edit::get_string(&text, "critical_scenario"),
            has_token: token.is_some(),
            token_where: token.map(|(_, w)| w.to_string()).unwrap_or_default(),
        }
    }

    fn check_hub(&self, hub_url: &str) -> Check {
        newsflash::setup::check_hub(hub_url)
    }

    fn check_token(&self, hub_url: &str, token: Option<&str>) -> Check {
        match token
            .map(str::to_string)
            .or_else(|| stored_token().map(|(t, _)| t))
        {
            Some(t) => newsflash::setup::check_token(hub_url, &t),
            None => Check::Rejected("no token — paste one first".into()),
        }
    }

    fn install(&self, choices: &Choices, log: &mut dyn FnMut(String)) -> Result<(), String> {
        let path = paths::config_path();
        let mut text =
            std::fs::read_to_string(&path).unwrap_or_else(|_| app::CONFIG_EXAMPLE.into());
        text = config_edit::set_string(&text, "hub_url", &choices.hub_url);
        text = config_edit::set_string(&text, "language", &choices.language);
        if let Some(scenario) = &choices.critical_scenario {
            text = config_edit::set_string(&text, "critical_scenario", scenario);
        }
        std::fs::create_dir_all(paths::config_dir())
            .map_err(|e| format!("creating {}: {e}", paths::config_dir().display()))?;
        std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
        log(format!("wrote {}", path.display()));
        if let Some(token) = &choices.token {
            secret::store(&paths::token_path(), token)?;
            // This process may have picked up the old token already.
            // SAFETY: environment access is synchronised on Windows.
            unsafe { std::env::set_var("KYU_TOKEN", token) };
            log("stored the token, encrypted for your Windows account (DPAPI)".into());
        }
        match installer::install(
            &installer::Options {
                add_to_path: choices.add_to_path,
                autostart: choices.autostart,
            },
            log,
        )? {
            installer::Installed::Started => Ok(()),
            installer::Installed::NeedsConfig => Err("the config could not be written".into()),
            installer::Installed::ConfigUnusable(remedy) => Err(remedy),
        }
    }

    fn uninstall(&self, log: &mut dyn FnMut(String)) -> Result<(), String> {
        installer::uninstall(log)
    }

    fn send_test(&self) -> Result<String, String> {
        let (config, _) = app::load_config()?;
        let msg = send_test::TestMessage {
            title: "newsflash setup".into(),
            message: "Werkt! Notificaties komen binnen via kyu.".into(),
            priority: "info".into(),
        };
        HubClient::new(&config)
            .publish(&send_test::build_envelope(&msg))
            .map(|id| format!("published test message {id} — it should pop up now"))
            .map_err(|e| format!("publish failed: {}", e.detail))
    }

    fn open_url(&self, url: &str) {
        unsafe {
            ShellExecuteW(
                None,
                &HSTRING::from("open"),
                &HSTRING::from(url),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

/// `newsflash setup [uninstall]`, and a first double-click.
pub fn run(start_uninstall: bool) -> i32 {
    match newsflash_setup::run(Arc::new(WinBackend), start_uninstall) {
        Ok(()) => 0,
        Err(e) => {
            crate::toast::show_notice("newsflash setup could not open", &e);
            eprintln!("{e}");
            1
        }
    }
}
