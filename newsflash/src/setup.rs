//! `newsflash setup` on Linux: the wizard (newsflash-setup) over this
//! crate's installer, config and hub client. Token handling follows
//! AR10 on Linux: when latch is installed and no new token is typed,
//! latch keeps providing it; a typed token goes to a 0600 `token_file`
//! and the unit then runs without latch (latch's env would override it).

use crate::config::{self, Config, DEFAULT_INTERACTIVE_WAIT_MARGIN_MS, DEFAULT_TTL_MINUTES};
use crate::config_edit;
use crate::hub_client::HubClient;
use crate::install::{self, Layout, Options};
use crate::send_test;
use courier_core::toast::Language;
use newsflash_setup::{Backend, Check, Choices, Existing};
use std::path::PathBuf;
use std::sync::Arc;

pub struct LinuxBackend {
    layout: Layout,
}

impl LinuxBackend {
    /// Config (comments kept) + typed token (0600 file), validated the
    /// way the service will load it.
    fn write_config(&self, choices: &Choices, log: &mut dyn FnMut(String)) -> Result<(), String> {
        let path = self.layout.user_config();
        let mut text = self
            .config_text()
            .unwrap_or_else(|| install::CONFIG_EXAMPLE.to_string());
        text = config_edit::set_string(&text, "hub_url", &choices.hub_url);
        text = config_edit::set_string(&text, "language", &choices.language);
        if let Some(token) = &choices.token {
            let file = self.token_file();
            write_private(&file, token)?;
            text = config_edit::set_string(&text, "token_file", &file.display().to_string());
            log(format!(
                "stored the token in {} (mode 0600)",
                file.display()
            ));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, &text).map_err(|e| format!("writing {}: {e}", path.display()))?;
        log(format!("wrote {}", path.display()));
        // Validate like the service will, unless the token only lives
        // in latch (not visible to this process).
        if choices.token.is_some() || self.stored_token().is_some() {
            config::load(&path)?;
        }
        Ok(())
    }

    fn config_text(&self) -> Option<String> {
        std::fs::read_to_string(self.layout.user_config()).ok()
    }

    fn token_file(&self) -> PathBuf {
        self.layout.config_dir.join("newsflash").join("token")
    }

    /// The token newsflash would use right now, if this process can see
    /// it (latch's cannot be read from here).
    fn stored_token(&self) -> Option<String> {
        if let Ok(t) = std::env::var("KYU_TOKEN")
            && !t.trim().is_empty()
        {
            return Some(t.trim().to_string());
        }
        let path = self
            .config_text()
            .and_then(|t| config_edit::get_string(&t, "token_file"))?;
        let t = std::fs::read_to_string(path).ok()?;
        Some(t.trim().to_string()).filter(|t| !t.is_empty())
    }
}

/// A client for checks only: defaults for everything but hub and token.
fn probe_client(hub_url: &str, token: &str) -> HubClient {
    HubClient::new(&Config {
        hub_url: hub_url.trim_end_matches('/').to_string(),
        topic: "notify.kenny".into(),
        subscription: "desktop".into(),
        language: Language::Nl,
        ttl_ms: DEFAULT_TTL_MINUTES * 60_000,
        sound_file: None,
        token: token.to_string(),
        interactive_wait_margin_ms: DEFAULT_INTERACTIVE_WAIT_MARGIN_MS,
        snooze_minutes: crate::snooze::DEFAULT_SNOOZE_MINUTES,
        lifetimes: Default::default(),
        popup: Default::default(),
        link_base_url: None,
    })
}

/// Shared by both platforms' wizards: any HTTP answer proves the hub.
pub fn check_hub(hub_url: &str) -> Check {
    match probe_client(hub_url, "reachability-probe").check_access() {
        Ok(()) => Check::Ok("hub reachable".into()),
        Err(e) if e.status.is_some() => Check::Ok("hub reachable".into()),
        Err(e) => Check::Unreachable(format!("no answer from {hub_url}: {}", e.detail)),
    }
}

/// Shared by both platforms' wizards.
pub fn check_token(hub_url: &str, token: &str) -> Check {
    match probe_client(hub_url, token).check_access() {
        Ok(()) => Check::Ok("token accepted".into()),
        Err(e) if matches!(e.status, Some(401 | 403)) => Check::Rejected(
            "the hub rejected this token — check it, or create a new one on the /apps page".into(),
        ),
        Err(e) if e.status.is_none() => {
            Check::Unreachable(format!("the hub did not answer: {}", e.detail))
        }
        Err(e) => Check::Rejected(format!(
            "the hub answered {} — {}",
            e.status.unwrap_or(0),
            e.detail
        )),
    }
}

impl Backend for LinuxBackend {
    fn os_name(&self) -> &'static str {
        "Linux"
    }

    fn has_critical_scenario(&self) -> bool {
        false
    }

    fn detect(&self) -> Existing {
        let text = self.config_text().unwrap_or_default();
        let latch = install::find_latch(&self.layout);
        let unit_uses_latch =
            std::fs::read_to_string(self.layout.unit_path()).is_ok_and(|u| u.contains(" run -- "));
        let stored = self.stored_token().is_some();
        Existing {
            installed: self.layout.unit_path().is_file(),
            running: install::systemctl(&["is-active", "--quiet", "newsflash"]).is_ok(),
            hub_url: config_edit::get_string(&text, "hub_url"),
            language: config_edit::get_string(&text, "language"),
            critical_scenario: None,
            has_token: stored || (latch.is_some() && unit_uses_latch),
            token_where: if stored {
                "token file".into()
            } else {
                "latch".into()
            },
        }
    }

    fn check_hub(&self, hub_url: &str) -> Check {
        check_hub(hub_url)
    }

    fn check_token(&self, hub_url: &str, token: Option<&str>) -> Check {
        match token.map(str::to_string).or_else(|| self.stored_token()) {
            Some(t) => check_token(hub_url, &t),
            // Held by latch, unreadable from here: the service will tell.
            None => Check::Ok("keeping the token in latch".into()),
        }
    }

    fn install(&self, choices: &Choices, log: &mut dyn FnMut(String)) -> Result<(), String> {
        self.write_config(choices, log)?;
        install::install_with(
            &self.layout,
            &Options {
                add_to_path: choices.add_to_path,
                autostart: choices.autostart,
                use_latch: choices.token.is_none(),
            },
            log,
        )
    }

    fn uninstall(&self, log: &mut dyn FnMut(String)) -> Result<(), String> {
        install::uninstall_with(&self.layout, log)
    }

    fn send_test(&self) -> Result<String, String> {
        let msg = send_test::TestMessage {
            title: "newsflash setup".into(),
            message: "Werkt! Notificaties komen binnen via kyu.".into(),
            priority: "info".into(),
        };
        if let Ok(cfg) = config::load(&self.layout.user_config()) {
            return HubClient::new(&cfg)
                .publish(&send_test::build_envelope(&msg))
                .map(|id| format!("published test message {id} — it should pop up now"))
                .map_err(|e| format!("publish failed: {}", e.detail));
        }
        // Token held by latch: let latch run the send-test for us.
        let latch = install::find_latch(&self.layout).ok_or("no token available to send with")?;
        let out = std::process::Command::new(latch)
            .args(["run", "--"])
            .arg(self.layout.installed_bin())
            .arg("send-test")
            .current_dir(self.layout.home.join("Projects").join("newsflash"))
            .output()
            .map_err(|e| format!("running latch: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    fn open_url(&self, url: &str) {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn write_private(path: &std::path::Path, secret: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("writing {}: {e}", path.display()))?;
        f.write_all(secret.as_bytes())
            .map_err(|e| format!("writing {}: {e}", path.display()))?;
        // An existing file keeps its old mode on open; force 0600 (AR10).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", path.display()))
    }
    #[cfg(not(unix))]
    std::fs::write(path, secret).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// `newsflash setup [uninstall]`.
pub fn run(start_uninstall: bool) -> i32 {
    let backend = Arc::new(LinuxBackend {
        layout: Layout::from_env(),
    });
    match newsflash_setup::run(backend, start_uninstall) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}\n(no display? use `newsflash install` in a terminal instead)");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub(status: u16) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", server.server_addr());
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let _ =
                    req.respond(tiny_http::Response::from_string("{}").with_status_code(status));
            }
        });
        addr
    }

    #[test]
    fn hub_check_counts_any_http_answer_and_token_check_tells_rejection_apart() {
        let open = hub(200);
        let locked = hub(401);
        assert_eq!(check_hub(&open), Check::Ok("hub reachable".into()));
        assert_eq!(check_hub(&locked), Check::Ok("hub reachable".into()));
        assert_eq!(check_token(&open, "t"), Check::Ok("token accepted".into()));
        assert!(matches!(check_token(&locked, "t"), Check::Rejected(_)));
        // A topic nobody published to yet (404) is still a working token.
        assert_eq!(
            check_token(&hub(404), "t"),
            Check::Ok("token accepted".into())
        );
        assert!(matches!(
            check_hub("http://127.0.0.1:9"),
            Check::Unreachable(_)
        ));
    }

    #[test]
    fn the_wizard_writes_config_and_a_private_token_file_keeping_comments() {
        let root = std::env::temp_dir().join(format!("nf-setup-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let backend = LinuxBackend {
            layout: Layout {
                home: root.clone(),
                bin_dir: root.join("bin"),
                config_dir: root.join("config"),
                data_dir: root.join("data"),
            },
        };
        let choices = Choices {
            hub_url: "http://10.10.10.9:8080".into(),
            token: Some("wizard-token".into()),
            language: "en".into(),
            critical_scenario: None,
            autostart: true,
            add_to_path: true,
        };
        let mut lines = Vec::new();
        backend
            .write_config(&choices, &mut |l| lines.push(l))
            .unwrap();
        let text = std::fs::read_to_string(backend.layout.user_config()).unwrap();
        assert!(text.contains("hub_url = \"http://10.10.10.9:8080\""));
        assert!(text.contains("\nlanguage = \"en\""));
        assert!(text.contains("# newsflash configuration"), "comments kept");
        let token_file = root.join("config/newsflash/token");
        assert_eq!(
            std::fs::read_to_string(&token_file).unwrap(),
            "wizard-token"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&token_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // What the service would load is what the wizard chose.
        let cfg = config::load(&backend.layout.user_config()).unwrap();
        assert_eq!(cfg.hub_url, "http://10.10.10.9:8080");
        assert_eq!(cfg.token, "wizard-token");
        assert!(
            !lines.iter().any(|l| l.contains("wizard-token")),
            "token never logged"
        );
    }
}
