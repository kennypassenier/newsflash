//! The wizard driven headlessly (egui_kittest): real pages, real
//! buttons and text fields, a fake platform that records what the
//! wizard asked it to do.

use super::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[derive(Default)]
struct Fake {
    existing: Existing,
    windows: bool,
    hub_check: Option<Check>,
    token_check: Option<Check>,
    checks: Mutex<Vec<(String, Option<String>)>>,
    installed: Mutex<Option<Choices>>,
    uninstalled: Mutex<bool>,
    tests_sent: Mutex<u32>,
}

impl Backend for Fake {
    fn os_name(&self) -> &'static str {
        if self.windows { "Windows" } else { "Linux" }
    }
    fn has_critical_scenario(&self) -> bool {
        self.windows
    }
    fn detect(&self) -> Existing {
        self.existing.clone()
    }
    fn check_hub(&self, hub_url: &str) -> Check {
        self.checks
            .lock()
            .unwrap()
            .push((hub_url.to_string(), None));
        self.hub_check.clone().unwrap_or(Check::Ok("fine".into()))
    }
    fn check_token(&self, hub_url: &str, token: Option<&str>) -> Check {
        self.checks
            .lock()
            .unwrap()
            .push((hub_url.to_string(), token.map(str::to_string)));
        self.token_check.clone().unwrap_or(Check::Ok("fine".into()))
    }
    fn install(&self, choices: &Choices, log: &mut dyn FnMut(String)) -> Result<(), String> {
        log("copied the binary".into());
        *self.installed.lock().unwrap() = Some(choices.clone());
        Ok(())
    }
    fn uninstall(&self, log: &mut dyn FnMut(String)) -> Result<(), String> {
        log("removed".into());
        *self.uninstalled.lock().unwrap() = true;
        Ok(())
    }
    fn send_test(&self) -> Result<String, String> {
        *self.tests_sent.lock().unwrap() += 1;
        Ok("published test message 42".into())
    }
    fn open_url(&self, _url: &str) {}
}

fn harness(fake: Arc<Fake>, start_uninstall: bool) -> Harness<'static, Wizard> {
    let mut wizard = Wizard::new(fake, start_uninstall);
    wizard.sync = true;
    Harness::builder()
        .with_size(egui::vec2(560.0, 420.0))
        .build_ui_state(|ui, w: &mut Wizard| w.show(ui), wizard)
}

fn click(h: &mut Harness<'static, Wizard>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn type_token(h: &mut Harness<'static, Wizard>, token: &str) {
    let field = h.get_by_role(Role::PasswordInput);
    field.focus();
    field.type_text(token);
    h.run();
}

#[test]
fn a_fresh_install_walks_every_page_and_installs_what_was_chosen() {
    let fake = Arc::new(Fake {
        windows: true,
        ..Default::default()
    });
    let mut h = harness(Arc::clone(&fake), false);
    h.run();
    assert!(
        h.query_by_label("Not installed on this computer yet.")
            .is_some()
    );

    click(&mut h, "Install");
    assert_eq!(h.state().page, Page::Hub);
    click(&mut h, "Next");
    assert_eq!(h.state().page, Page::Token);
    assert_eq!(
        fake.checks.lock().unwrap()[0],
        (DEFAULT_HUB.to_string(), None)
    );

    // No stored token: Next stays disabled until one is typed.
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(h.state().page, Page::Token);
    type_token(&mut h, "secret-token");
    click(&mut h, "Next");
    assert_eq!(h.state().page, Page::Options);
    assert_eq!(
        fake.checks.lock().unwrap()[1].1.as_deref(),
        Some("secret-token")
    );

    click(&mut h, "English");
    click(&mut h, "Break through Do Not Disturb");
    click(&mut h, "Install");
    assert_eq!(h.state().page, Page::Done(Job::Install));
    assert_eq!(
        fake.installed.lock().unwrap().clone().unwrap(),
        Choices {
            hub_url: DEFAULT_HUB.into(),
            token: Some("secret-token".into()),
            language: "en".into(),
            critical_scenario: Some("urgent".into()),
            autostart: true,
            add_to_path: true,
        }
    );
    assert!(
        h.query_by_label("✔ newsflash is installed and running.")
            .is_some()
    );

    click(&mut h, "Send a test notification");
    assert_eq!(*fake.tests_sent.lock().unwrap(), 1);
    assert!(h.query_by_label("published test message 42").is_some());
}

#[test]
fn a_rejected_token_blocks_and_offers_no_way_around() {
    let fake = Arc::new(Fake {
        token_check: Some(Check::Rejected("the hub rejected this token".into())),
        ..Default::default()
    });
    let mut h = harness(Arc::clone(&fake), false);
    h.run();
    click(&mut h, "Install");
    click(&mut h, "Next");
    type_token(&mut h, "wrong");
    click(&mut h, "Next");
    assert_eq!(h.state().page, Page::Token);
    assert!(h.query_by_label("the hub rejected this token").is_some());
    assert!(h.query_by_label("Continue anyway").is_none());
    assert!(fake.installed.lock().unwrap().is_none());
}

#[test]
fn an_unreachable_hub_can_be_continued_past() {
    let fake = Arc::new(Fake {
        hub_check: Some(Check::Unreachable("no answer from the hub".into())),
        ..Default::default()
    });
    let mut h = harness(fake, false);
    h.run();
    click(&mut h, "Install");
    click(&mut h, "Next");
    assert_eq!(h.state().page, Page::Hub);
    assert!(h.query_by_label("no answer from the hub").is_some());
    click(&mut h, "Continue anyway");
    assert_eq!(h.state().page, Page::Token);
}

#[test]
fn an_existing_install_keeps_its_token_and_values_and_offers_update() {
    let fake = Arc::new(Fake {
        existing: Existing {
            installed: true,
            running: true,
            hub_url: Some("http://hub.lan:9000".into()),
            language: Some("en".into()),
            has_token: true,
            token_where: "latch".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut h = harness(Arc::clone(&fake), false);
    h.run();
    assert!(h.query_by_label("Installed and running.").is_some());
    click(&mut h, "Update / reconfigure");
    click(&mut h, "Next");
    click(&mut h, "Next"); // empty token = keep the stored one
    assert_eq!(h.state().page, Page::Options);
    assert!(
        h.query_by_label("Stay on screen until I answer (recommended)")
            .is_none(),
        "no critical-scenario choice on Linux"
    );
    click(&mut h, "Update");
    let chosen = fake.installed.lock().unwrap().clone().unwrap();
    assert_eq!(chosen.hub_url, "http://hub.lan:9000");
    assert_eq!(chosen.token, None);
    assert_eq!(chosen.language, "en");
    assert_eq!(chosen.critical_scenario, None);
}

#[test]
fn uninstall_asks_first_then_uninstalls() {
    let fake = Arc::new(Fake {
        existing: Existing {
            installed: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let mut h = harness(Arc::clone(&fake), false);
    h.run();
    click(&mut h, "Uninstall");
    assert_eq!(h.state().page, Page::ConfirmUninstall);
    assert!(!*fake.uninstalled.lock().unwrap());
    click(&mut h, "Uninstall");
    assert!(*fake.uninstalled.lock().unwrap());
    assert!(h.query_by_label("✔ newsflash is uninstalled.").is_some());
}

#[test]
fn the_apps_list_uninstall_entry_opens_straight_on_the_confirmation() {
    let fake = Arc::new(Fake {
        existing: Existing {
            installed: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let h = harness(fake, true);
    assert_eq!(h.state().page, Page::ConfirmUninstall);
}

#[test]
fn a_hub_address_without_http_cannot_proceed() {
    let fake = Arc::new(Fake::default());
    let mut h = harness(Arc::clone(&fake), false);
    h.run();
    click(&mut h, "Install");
    h.state_mut().hub_url = "https://hub.lan".into();
    h.run();
    assert!(h.query_by_label("Must start with http://").is_some());
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(h.state().page, Page::Hub);
    assert!(fake.checks.lock().unwrap().is_empty());
}
