//! The newsflash setup wizard: one window, the same pages on Windows
//! and Linux (docs/SETUP.md). Welcome → hub → token → options → install
//! → done, plus an uninstall path. Every check that can fail is made
//! here, before anything is written: the hub must answer, the token
//! must be accepted.
//!
//! This crate is UI only. Each platform binary implements [`Backend`]
//! with its own installer (newsflash on Linux: systemd, PATH drop-ins;
//! newsflash-win: registry, COM activator, DPAPI token), so the wizard
//! never needs to know which OS it runs on beyond `os_name`.
//!
//! Slow calls (hub checks, installing) run on a worker thread so the
//! window never freezes; tests switch that off (`sync`) and drive the
//! pages headlessly with egui_kittest.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

/// What the platform found before the wizard opened.
#[derive(Debug, Clone, Default)]
pub struct Existing {
    pub installed: bool,
    pub running: bool,
    pub hub_url: Option<String>,
    pub language: Option<String>,
    pub critical_scenario: Option<String>,
    /// A token is already stored (the token page may be left empty).
    pub has_token: bool,
    /// Where the token is kept, in words ("DPAPI store", "latch", …).
    pub token_where: String,
}

/// The outcome of a hub check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Ok(String),
    /// Transport trouble — worth a "continue anyway".
    Unreachable(String),
    /// 401/403 — never continue with this token.
    Rejected(String),
}

/// Everything the user chose; handed to [`Backend::install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub hub_url: String,
    /// `None` = keep the stored token.
    pub token: Option<String>,
    pub language: String,
    /// Windows only (`None` elsewhere): "reminder" | "urgent" | "alarm".
    pub critical_scenario: Option<String>,
    pub autostart: bool,
    pub add_to_path: bool,
}

pub trait Backend: Send + Sync + 'static {
    /// "Windows" / "Linux" — used in wording only.
    fn os_name(&self) -> &'static str;
    /// Whether the critical-scenario choice applies (Windows).
    fn has_critical_scenario(&self) -> bool;
    fn detect(&self) -> Existing;
    /// Does anything answer at `hub_url`? (Any HTTP status counts —
    /// a 401 still proves the hub is there.)
    fn check_hub(&self, hub_url: &str) -> Check;
    /// Is the token accepted? `None` = check the stored token.
    fn check_token(&self, hub_url: &str, token: Option<&str>) -> Check;
    fn install(&self, choices: &Choices, log: &mut dyn FnMut(String)) -> Result<(), String>;
    fn uninstall(&self, log: &mut dyn FnMut(String)) -> Result<(), String>;
    fn send_test(&self) -> Result<String, String>;
    fn open_url(&self, url: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Install,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Welcome,
    Hub,
    Token,
    Options,
    Working(Job),
    Done(Job),
    ConfirmUninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Task {
    CheckHub,
    CheckToken,
    Install,
    Uninstall,
    SendTest,
}

enum TaskResult {
    Check(Check),
    Finished(Result<(), String>),
    Sent(Result<String, String>),
}

pub const DEFAULT_HUB: &str = "http://10.10.10.9:8080";

pub struct Wizard {
    backend: Arc<dyn Backend>,
    pub page: Page,
    existing: Existing,
    hub_url: String,
    token: String,
    show_token: bool,
    language: String,
    critical: String,
    autostart: bool,
    add_to_path: bool,
    /// Last check/test result: (good?, text).
    status: Option<(bool, String)>,
    /// A failed-but-transient check may be overridden.
    allow_continue: bool,
    pending: Option<(Task, Receiver<TaskResult>)>,
    log: Arc<Mutex<Vec<String>>>,
    outcome: Option<Result<(), String>>,
    test_result: Option<(bool, String)>,
    /// Run tasks inline (tests); the real window uses threads.
    pub sync: bool,
}

impl Wizard {
    pub fn new(backend: Arc<dyn Backend>, start_uninstall: bool) -> Self {
        let existing = backend.detect();
        Wizard {
            hub_url: existing
                .hub_url
                .clone()
                .filter(|u| !u.contains("127.0.0.1"))
                .unwrap_or_else(|| DEFAULT_HUB.to_string()),
            language: existing.language.clone().unwrap_or_else(|| "nl".into()),
            critical: existing
                .critical_scenario
                .clone()
                .unwrap_or_else(|| "reminder".into()),
            page: if start_uninstall && existing.installed {
                Page::ConfirmUninstall
            } else {
                Page::Welcome
            },
            existing,
            backend,
            token: String::new(),
            show_token: false,
            autostart: true,
            add_to_path: true,
            status: None,
            allow_continue: false,
            pending: None,
            log: Arc::new(Mutex::new(Vec::new())),
            outcome: None,
            test_result: None,
            sync: false,
        }
    }

    fn choices(&self) -> Choices {
        Choices {
            hub_url: self.hub_url.trim().trim_end_matches('/').to_string(),
            token: Some(self.token.trim().to_string()).filter(|t| !t.is_empty()),
            language: self.language.clone(),
            critical_scenario: self
                .backend
                .has_critical_scenario()
                .then(|| self.critical.clone()),
            autostart: self.autostart,
            add_to_path: self.add_to_path,
        }
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        self.status = None;
        self.allow_continue = false;
    }

    fn start(&mut self, task: Task) {
        let backend = Arc::clone(&self.backend);
        let log = Arc::clone(&self.log);
        let choices = self.choices();
        let job = move || -> TaskResult {
            let mut push = |line: String| {
                if let Ok(mut l) = log.lock() {
                    l.push(line);
                }
            };
            match task {
                Task::CheckHub => TaskResult::Check(backend.check_hub(&choices.hub_url)),
                Task::CheckToken => TaskResult::Check(
                    backend.check_token(&choices.hub_url, choices.token.as_deref()),
                ),
                Task::Install => TaskResult::Finished(backend.install(&choices, &mut push)),
                Task::Uninstall => TaskResult::Finished(backend.uninstall(&mut push)),
                Task::SendTest => TaskResult::Sent(backend.send_test()),
            }
        };
        if self.sync {
            let result = job();
            self.finish(task, result);
        } else {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(job());
            });
            self.pending = Some((task, rx));
        }
    }

    fn poll(&mut self) {
        let Some((task, rx)) = &self.pending else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            let task = *task;
            self.pending = None;
            self.finish(task, result);
        }
    }

    fn finish(&mut self, task: Task, result: TaskResult) {
        match (task, result) {
            (Task::CheckHub | Task::CheckToken, TaskResult::Check(check)) => {
                let next = if task == Task::CheckHub {
                    Page::Token
                } else {
                    Page::Options
                };
                match check {
                    Check::Ok(_) => self.go(next),
                    Check::Unreachable(e) => {
                        self.status = Some((false, e));
                        self.allow_continue = true;
                    }
                    Check::Rejected(e) => {
                        self.status = Some((false, e));
                        self.allow_continue = false;
                    }
                }
            }
            (Task::Install, TaskResult::Finished(r)) => {
                self.outcome = Some(r);
                self.go(Page::Done(Job::Install));
            }
            (Task::Uninstall, TaskResult::Finished(r)) => {
                self.outcome = Some(r);
                self.go(Page::Done(Job::Uninstall));
            }
            (Task::SendTest, TaskResult::Sent(r)) => {
                self.test_result = Some(match r {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
            _ => {}
        }
    }

    fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if self.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.horizontal(|ui| {
            ui.heading("newsflash setup");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(self.step_label());
            });
        });
        ui.separator();
        match self.page {
            Page::Welcome => self.welcome(ui),
            Page::Hub => self.hub(ui),
            Page::Token => self.token_page(ui),
            Page::Options => self.options(ui),
            Page::Working(job) => self.working(ui, job),
            Page::Done(job) => self.done(ui, job),
            Page::ConfirmUninstall => self.confirm_uninstall(ui),
        }
    }

    fn step_label(&self) -> &'static str {
        match self.page {
            Page::Welcome => "",
            Page::Hub => "step 1 of 3 · hub",
            Page::Token => "step 2 of 3 · token",
            Page::Options => "step 3 of 3 · options",
            Page::Working(_) => "working…",
            Page::Done(_) => "done",
            Page::ConfirmUninstall => "uninstall",
        }
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("checking…");
            });
        } else if let Some((good, text)) = &self.status {
            let color = if *good {
                egui::Color32::from_rgb(34, 160, 90)
            } else {
                egui::Color32::from_rgb(210, 60, 60)
            };
            ui.colored_label(color, text);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let os = self.backend.os_name();
        ui.label(format!(
            "newsflash shows your kyu notifications (topic notify.kenny) as {os} \
             notifications, with action buttons."
        ));
        ui.label(
            "Garuda and Windows share the same kyu subscription: whichever one you \
             are logged in to receives the notifications.",
        );
        ui.add_space(4.0);
        let state = match (self.existing.installed, self.existing.running) {
            (false, _) => "Not installed on this computer yet.".to_string(),
            (true, true) => "Installed and running.".to_string(),
            (true, false) => "Installed, but not running right now.".to_string(),
        };
        ui.strong(state);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let label = if self.existing.installed {
                "Update / reconfigure"
            } else {
                "Install"
            };
            if ui.button(label).clicked() {
                self.go(Page::Hub);
            }
            if self.existing.installed && ui.button("Uninstall").clicked() {
                self.go(Page::ConfirmUninstall);
            }
            if ui.button("Close").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn hub(&mut self, ui: &mut egui::Ui) {
        ui.label("Address of your kyu hub:");
        ui.add(
            egui::TextEdit::singleline(&mut self.hub_url)
                .desired_width(f32::INFINITY)
                .hint_text(DEFAULT_HUB),
        );
        ui.weak("Plain http on your LAN — newsflash deliberately ships no TLS.");
        self.status_line(ui);
        let valid = self.hub_url.trim().starts_with("http://");
        if !valid && !self.hub_url.trim().is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(210, 60, 60),
                "Must start with http://",
            );
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.go(Page::Welcome);
            }
            if ui
                .add_enabled(valid && !self.busy(), egui::Button::new("Next"))
                .clicked()
            {
                self.start(Task::CheckHub);
            }
            if self.allow_continue && ui.button("Continue anyway").clicked() {
                self.go(Page::Token);
            }
        });
    }

    fn token_page(&mut self, ui: &mut egui::Ui) {
        ui.label("App token for newsflash:");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.token)
                    .password(!self.show_token)
                    .desired_width(320.0)
                    .hint_text(if self.existing.has_token {
                        "leave empty to keep the stored token"
                    } else {
                        "paste the token here"
                    }),
            );
            ui.checkbox(&mut self.show_token, "Show");
        });
        let apps = format!("{}/apps", self.hub_url.trim().trim_end_matches('/'));
        ui.horizontal(|ui| {
            ui.label("No token yet? Create one named");
            ui.code(format!(
                "newsflash-{}",
                self.backend.os_name().to_lowercase()
            ));
            ui.label("on");
            if ui.link(&apps).clicked() {
                self.backend.open_url(&apps);
            }
        });
        if self.existing.has_token {
            ui.weak(format!("Stored token: {}.", self.existing.token_where));
        }
        self.status_line(ui);
        let missing = self.token.trim().is_empty() && !self.existing.has_token;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.go(Page::Hub);
            }
            if ui
                .add_enabled(!missing && !self.busy(), egui::Button::new("Next"))
                .clicked()
            {
                self.start(Task::CheckToken);
            }
            if self.allow_continue && ui.button("Continue anyway").clicked() {
                self.go(Page::Options);
            }
        });
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui.label("Language of the notifications:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.language, "nl".to_string(), "Nederlands");
            ui.radio_value(&mut self.language, "en".to_string(), "English");
        });
        if self.backend.has_critical_scenario() {
            ui.add_space(4.0);
            ui.label("Critical notifications:");
            ui.radio_value(
                &mut self.critical,
                "reminder".to_string(),
                "Stay on screen until I answer (recommended)",
            );
            ui.radio_value(
                &mut self.critical,
                "urgent".to_string(),
                "Break through Do Not Disturb",
            );
            ui.radio_value(
                &mut self.critical,
                "alarm".to_string(),
                "Stay on screen with a looping alarm sound",
            );
        }
        ui.add_space(4.0);
        ui.checkbox(&mut self.autostart, "Start automatically when I log in");
        ui.checkbox(
            &mut self.add_to_path,
            "Add newsflash to PATH (type `newsflash` in a terminal)",
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.go(Page::Token);
            }
            let label = if self.existing.installed {
                "Update"
            } else {
                "Install"
            };
            if ui.button(label).clicked() {
                self.log.lock().map(|mut l| l.clear()).ok();
                self.go(Page::Working(Job::Install));
                self.start(Task::Install);
            }
        });
    }

    fn log_view(&self, ui: &mut egui::Ui) {
        let lines = self.log.lock().map(|l| l.clone()).unwrap_or_default();
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in lines {
                    ui.monospace(line);
                }
            });
    }

    fn working(&mut self, ui: &mut egui::Ui, job: Job) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(match job {
                Job::Install => "Installing…",
                Job::Uninstall => "Uninstalling…",
            });
        });
        self.log_view(ui);
    }

    fn done(&mut self, ui: &mut egui::Ui, job: Job) {
        match (&self.outcome, job) {
            (Some(Ok(())), Job::Install) => {
                ui.strong("✔ newsflash is installed and running.");
                if self.autostart {
                    ui.label("It starts by itself every time you log in.");
                }
                if self.add_to_path {
                    ui.label("Open a new terminal to use the `newsflash` command.");
                }
            }
            (Some(Ok(())), Job::Uninstall) => {
                ui.strong("✔ newsflash is uninstalled.");
                ui.label("Your config and token were kept, in case you reinstall.");
            }
            (Some(Err(e)), _) => {
                ui.colored_label(egui::Color32::from_rgb(210, 60, 60), format!("✖ {e}"));
            }
            (None, _) => {}
        }
        egui::CollapsingHeader::new("Details")
            .default_open(matches!(self.outcome, Some(Err(_))))
            .show(ui, |ui| self.log_view(ui));
        if let Some((good, text)) = &self.test_result {
            let color = if *good {
                egui::Color32::from_rgb(34, 160, 90)
            } else {
                egui::Color32::from_rgb(210, 60, 60)
            };
            ui.colored_label(color, text);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if matches!(self.outcome, Some(Ok(()))) && job == Job::Install {
                if self.busy() {
                    ui.spinner();
                } else if ui.button("Send a test notification").clicked() {
                    self.start(Task::SendTest);
                }
            }
            if matches!(self.outcome, Some(Err(_))) && ui.button("Back").clicked() {
                self.go(if job == Job::Install {
                    Page::Options
                } else {
                    Page::Welcome
                });
            }
            if ui.button("Close").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn confirm_uninstall(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "This stops newsflash and removes it from startup, from PATH and from the \
             menu. Your config and token are kept.",
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.go(Page::Welcome);
            }
            if ui.button("Uninstall").clicked() {
                self.log.lock().map(|mut l| l.clear()).ok();
                self.go(Page::Working(Job::Uninstall));
                self.start(Task::Uninstall);
            }
        });
    }
}

impl eframe::App for Wizard {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| self.show(ui));
    }
}

/// Opens the wizard window and blocks until it is closed.
pub fn run(backend: Arc<dyn Backend>, start_uninstall: bool) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("newsflash setup")
            .with_app_id("newsflash-setup")
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([480.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "newsflash setup",
        options,
        Box::new(move |_cc| Ok(Box::new(Wizard::new(backend, start_uninstall)))),
    )
    .map_err(|e| format!("could not open the setup window: {e}"))
}

#[cfg(test)]
mod tests;
