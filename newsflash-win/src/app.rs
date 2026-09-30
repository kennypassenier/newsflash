//! Entry point shared by `newsflash.exe` (console: CLI + foreground
//! debugging) and `newsflashw.exe` (windowless: logon autostart and COM
//! activation). See docs/WINDOWS.md for the operator's view.

use crate::activator::{self, Click};
use crate::{instance, paths, registry, secret, toast, winconfig};
use courier_core::envelope::parse_envelope;
use courier_core::wintoast::{Activation, decode_activation};
use newsflash::config::{self, Config};
use newsflash::hub_client::HubClient;
use newsflash::logx;
use newsflash::run::{publish_action_result, run_with};
use newsflash::send_test;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const USAGE: &str = "usage:
  newsflash [run]                  run the courier in the foreground (logs to the console too)
  newsflash setup                  the install wizard (a window) — also: double-click newsflashw.exe
  newsflash install                copy to %LOCALAPPDATA%\\Programs\\newsflash, add it to PATH,
                                   register with Windows, start at logon, start now
  newsflash uninstall              stop, unregister, remove the logon autostart and PATH entry
  newsflash stop                   stop the running courier
  newsflash status                 registration, daemon, notification setting, config, log tail
  newsflash set-token              store the kyu app token (DPAPI-encrypted, read from stdin)
  newsflash clear-token            delete the stored token
  newsflash send-test [--title T] [--message M] [--priority info|warning|critical]
  newsflash send-json <file|->     publish a hand-written envelope (validated first)
  newsflash demo                   show local demo toasts of every Windows feature (no hub)
  newsflash --version";

/// Bundled images, written to %LOCALAPPDATA%\newsflash\assets.
const ASSETS: &[(&str, &[u8])] = &[
    ("app.png", include_bytes!("../assets/app.png")),
    ("info.png", include_bytes!("../assets/info.png")),
    ("warning.png", include_bytes!("../assets/warning.png")),
    ("critical.png", include_bytes!("../assets/critical.png")),
    ("demo-hero.png", include_bytes!("../assets/demo-hero.png")),
];

pub(crate) const CONFIG_EXAMPLE: &str = include_str!("../config.example.toml");

pub fn main(windowless: bool) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    // CLI commands print plain lines; the daemon and the COM launch
    // replace this with the log file sink (first set wins, so this is
    // only installed when neither of those runs).
    if !matches!(first, None | Some("run")) && !is_com_launch(first) {
        logx::set_sink(Box::new(|_, msg| eprintln!("{msg}")));
    }
    match first {
        Some("--version" | "-V") => {
            println!("newsflash {} (windows)", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            0
        }
        // A double-click on newsflashw.exe before anything is installed
        // opens the wizard; once registered, no-args is the daemon (the
        // logon autostart runs exactly that).
        None if windowless && !registry::is_registered() => crate::setup::run(false),
        None | Some("run") => daemon(!windowless),
        Some("setup") => crate::setup::run(args.get(1).map(String::as_str) == Some("uninstall")),
        // COM starts us with -Embedding for a click while no daemon runs.
        a if is_com_launch(a) => com_launch(),
        Some("install") => install(),
        Some("uninstall") => uninstall(),
        Some("stop") => stop(),
        Some("status") => status(),
        Some("set-token") => set_token(),
        Some("clear-token") => clear_token(),
        Some("send-test") => send_test_cmd(&args[1..]),
        Some("send-json") => send_json(args.get(1).map(String::as_str)),
        Some("demo") => crate::demo::run(),
        Some(other) => {
            eprintln!("unknown argument {other:?}\n{USAGE}");
            2
        }
    }
}

fn is_com_launch(arg: Option<&str>) -> bool {
    arg.is_some_and(|a| {
        a.eq_ignore_ascii_case("-embedding") || a.eq_ignore_ascii_case("/embedding")
    })
}

fn init_log(echo: bool) {
    let file = crate::logfile::LogFile::open(&paths::log_path(), echo);
    logx::set_sink(Box::new(move |priority, msg| file.write(priority, msg)));
}

pub fn log_stop_request() {
    logx::info("stop requested (newsflash stop / install) — finishing the current cycle");
}

/// AR10 order on Windows: KYU_TOKEN env → DPAPI store → token_file.
/// Must run before any thread starts (it may set the env var).
fn inject_stored_token() -> Result<&'static str, String> {
    if std::env::var_os("KYU_TOKEN").is_some() {
        return Ok("KYU_TOKEN environment variable");
    }
    match secret::load(&paths::token_path())? {
        Some(token) => {
            // SAFETY: called at startup before any other thread exists.
            unsafe { std::env::set_var("KYU_TOKEN", token) };
            Ok("DPAPI store (newsflash set-token)")
        }
        None => Ok("token_file from config (if set)"),
    }
}

pub(crate) fn load_config() -> Result<(Config, winconfig::WinConfig), String> {
    inject_stored_token()?;
    let path = paths::config_path();
    let config = config::load(&path)?;
    let win = winconfig::load(&path)?;
    Ok((config, win))
}

fn extract_assets(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, bytes) in ASSETS {
        let path = dir.join(name);
        let same = std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64);
        if !same {
            let _ = std::fs::write(&path, bytes);
        }
    }
}

/// The courier itself — `newsflash.exe` (echo to console) or the
/// windowless autostart.
fn daemon(echo: bool) -> i32 {
    init_log(echo);
    let Some(_lock) = instance::acquire() else {
        logx::info("another newsflash is already running in this session — exiting");
        if echo {
            eprintln!("newsflash is already running (see `newsflash status`).");
        }
        return 0;
    };
    let (config, win) = match load_config() {
        Ok(c) => c,
        Err(remedy) => {
            // AR8's fatal-config class. Without a console the log file
            // is easy to miss, so say it in a toast too.
            logx::error(&remedy);
            toast::show_notice("newsflash could not start", &remedy);
            return 1;
        }
    };
    extract_assets(&paths::assets_dir());

    let term = Arc::new(AtomicBool::new(false));
    // Ctrl+C in the console binary; the second press aborts (AR14).
    let _ = signal_hook::flag::register_conditional_shutdown(
        signal_hook::consts::SIGINT,
        130,
        Arc::clone(&term),
    );
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term));
    if let Err(e) = instance::watch_stop_event(Arc::clone(&term)) {
        logx::warn(&format!("{e} — `newsflash stop` will not work this run"));
    }

    let (tx, rx) = mpsc::channel();
    let _activator = match activator::register(tx) {
        Ok(r) => Some(r),
        Err(e) => {
            logx::warn(&format!(
                "{e} — toasts still show, but button clicks cannot be received this run"
            ));
            None
        }
    };
    let clicks_client = HubClient::new(&config);
    std::thread::spawn(move || {
        for click in rx {
            handle_click(&click, Some(&clicks_client), echo);
        }
    });

    logx::info(&format!(
        "windows: critical_scenario={:?} log={}",
        win.critical_scenario,
        paths::log_path().display()
    ));
    let mut desktop = toast::WinDesktop::new(
        &config,
        win.critical_scenario,
        paths::assets_dir(),
        paths::images_dir(),
    );
    run_with(&config, &mut desktop, &paths::state_path(), &term)
}

/// M10 reply path on Windows: decode what the button carried, publish.
pub fn handle_click(click: &Click, client: Option<&HubClient>, echo: bool) {
    let say = |line: String| {
        logx::info(&line);
        if echo {
            println!("{line}");
        }
    };
    match decode_activation(&click.args) {
        Some(Activation::Button {
            action_id,
            envelope_id,
            ack_id,
            hub_id,
            demo,
        }) => {
            // Input keys only: typed text may be private, the journal
            // keeps the shape, the hub gets the values.
            let keys: Vec<&str> = click.inputs.iter().map(|(k, _)| k.as_str()).collect();
            let with = if keys.is_empty() {
                String::new()
            } else {
                format!(" with inputs {keys:?}")
            };
            say(format!("{hub_id}: action {action_id:?} chosen{with}"));
            if demo {
                let values: Vec<String> = click
                    .inputs
                    .iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                say(format!(
                    "  demo toast — not published. inputs: [{}]",
                    values.join(", ")
                ));
                return;
            }
            if action_id == newsflash::snooze::SNOOZE_ACTION {
                let minutes = click
                    .inputs
                    .iter()
                    .find(|(k, _)| k == "snooze_minutes")
                    .and_then(|(_, v)| v.trim().parse().ok());
                match client {
                    Some(c) => newsflash::snooze::snooze_click(c, &hub_id, minutes),
                    None => logx::warn(&format!(
                        "{hub_id}: no usable config/token — the snooze could not be published"
                    )),
                }
            }
            match client {
                Some(c) => publish_action_result(
                    c,
                    &hub_id,
                    &envelope_id,
                    ack_id.as_deref(),
                    &action_id,
                    &click.inputs,
                ),
                None => logx::warn(&format!(
                    "{hub_id}: no usable config/token — the click could not be published"
                )),
            }
        }
        Some(Activation::Body { hub_id }) => {
            say(format!(
                "{hub_id}: toast body clicked (no click_url) — nothing to do"
            ));
        }
        Some(Activation::Header) => say("Notification Center header clicked".into()),
        None => logx::warn("a click with unrecognised arguments was ignored"),
    }
}

/// Windows started us to deliver a click while no daemon was running
/// (an old toast in Notification Center). Handle it and leave.
fn com_launch() -> i32 {
    init_log(false);
    let client = load_config()
        .map(|(c, _)| HubClient::new(&c))
        .map_err(|e| logx::warn(&format!("click arrived but config is unusable: {e}")))
        .ok();
    let (tx, rx) = mpsc::channel();
    let Ok(_registered) = activator::register(tx)
        .map_err(|e| logx::error(&format!("COM launch could not take the click: {e}")))
    else {
        return 1;
    };
    // The click arrives right after registration; linger briefly for
    // a second one, then exit.
    let mut timeout = Duration::from_secs(15);
    while let Ok(click) = rx.recv_timeout(timeout) {
        handle_click(&click, client.as_ref(), false);
        timeout = Duration::from_secs(3);
    }
    0
}

fn install() -> i32 {
    let mut say = |line: String| println!("{line}");
    match crate::installer::install(&crate::installer::Options::default(), &mut say) {
        Ok(crate::installer::Installed::Started) => {
            println!("try: newsflash send-test   (or: newsflash demo)");
            0
        }
        Ok(crate::installer::Installed::NeedsConfig) => {
            println!(
                "next:\n  1. set hub_url in that file\n  2. newsflash set-token\n  \
                 3. newsflash install   (again — starts it)\n\
                 or run `newsflash setup` for the wizard."
            );
            0
        }
        Ok(crate::installer::Installed::ConfigUnusable(remedy)) => {
            println!(
                "\nconfig is not usable yet: {remedy}\nfix it, then run `newsflash install` again."
            );
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Starts the windowless daemon fully detached. Raw `CreateProcessW`
/// with handle inheritance OFF: std's `Command` inherits every
/// inheritable handle, so the daemon would hold the caller's stdout
/// pipe open forever and `newsflash install | …` would never finish.
pub(crate) fn start_detached(exe: &Path) -> Result<(), String> {
    use windows::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    let mut cmdline: Vec<u16> = format!("\"{}\"", exe.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            &windows::core::HSTRING::from(exe.as_os_str()),
            Some(windows::core::PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            None,
            &windows::core::HSTRING::from(paths::install_dir().as_os_str()),
            &startup,
            &mut info,
        )
        .map_err(|e| e.message().to_string())?;
        let _ = windows::Win32::Foundation::CloseHandle(info.hThread);
        let _ = windows::Win32::Foundation::CloseHandle(info.hProcess);
    }
    Ok(())
}

fn uninstall() -> i32 {
    match crate::installer::uninstall(&mut |line| println!("{line}")) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn stop() -> i32 {
    if !instance::request_stop() {
        println!("newsflash is not running.");
        return 0;
    }
    if instance::wait_stopped(Duration::from_secs(45)) {
        println!("stopped.");
        0
    } else {
        eprintln!("asked it to stop, but it is still running after 45 s.");
        1
    }
}

fn status() -> i32 {
    println!("newsflash {} (windows)", env!("CARGO_PKG_VERSION"));
    let reg = registry::read();
    let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "— (missing)".into());
    println!("registered as        {}", show(&reg.display_name));
    println!("click activator      {}", show(&reg.activator_exe));
    println!("start at logon       {}", show(&reg.autostart));
    println!(
        "on PATH              {}",
        if registry::is_on_user_path(&paths::install_dir()) {
            "yes"
        } else {
            "no (newsflash install adds it)"
        }
    );
    println!(
        "daemon               {}",
        if instance::is_running() {
            "running"
        } else {
            "not running"
        }
    );
    let setting = toast::notifier()
        .and_then(|n| n.Setting().map_err(|e| e.message().to_string()))
        .map(|s| match s {
            windows::UI::Notifications::NotificationSetting::Enabled => "enabled".to_string(),
            other => format!("DISABLED ({other:?}) — Settings → System → Notifications"),
        })
        .unwrap_or_else(|e| format!("unknown ({e})"));
    println!("notifications        {setting}");
    println!(
        "fullscreen app       {}",
        if crate::fullscreen::app_in_front() {
            "in front — toasts wait silently (feat-10)"
        } else {
            "none"
        }
    );
    let token_source = inject_stored_token().unwrap_or("DPAPI store (unreadable!)");
    println!("token source         {token_source}");
    let config_path = paths::config_path();
    match config::load(&config_path).and_then(|c| winconfig::load(&config_path).map(|w| (c, w))) {
        Ok((c, w)) => println!(
            "config               {} — hub {} topic {} subscription {} ttl {}min language {:?} critical {:?}",
            config_path.display(),
            c.hub_url,
            c.topic,
            c.subscription,
            c.ttl_ms / 60_000,
            c.language,
            w.critical_scenario
        ),
        Err(e) => println!(
            "config               {} — NOT USABLE: {e}",
            config_path.display()
        ),
    }
    let log = paths::log_path();
    println!("log                  {}\n", log.display());
    for line in crate::logfile::tail(&log, 15) {
        println!("  {line}");
    }
    0
}

fn set_token() -> i32 {
    let token = secret::read_secret_line("kyu app token (input hidden): ");
    if token.is_empty() {
        eprintln!("no token given — nothing stored.");
        return 2;
    }
    match secret::store(&paths::token_path(), &token) {
        Ok(()) => {
            println!(
                "stored in {} (DPAPI, this Windows account only).",
                paths::token_path().display()
            );
            if instance::is_running() {
                println!("restart to use it: newsflash stop, then newsflash install");
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn clear_token() -> i32 {
    match std::fs::remove_file(paths::token_path()) {
        Ok(()) => {
            println!("stored token deleted.");
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no stored token.");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cli_config() -> Option<Config> {
    match load_config() {
        Ok((c, _)) => Some(c),
        Err(remedy) => {
            eprintln!("{remedy}");
            None
        }
    }
}

fn send_test_cmd(args: &[String]) -> i32 {
    let mut msg = send_test::TestMessage {
        title: "Testbericht".into(),
        message: "newsflash send-test (windows)".into(),
        priority: "info".into(),
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let slot = match arg.as_str() {
            "--title" => &mut msg.title,
            "--message" => &mut msg.message,
            "--priority" => &mut msg.priority,
            other => {
                eprintln!("unknown argument {other:?}\n{USAGE}");
                return 2;
            }
        };
        match it.next() {
            Some(v) => *slot = v.clone(),
            None => {
                eprintln!("{arg} needs a value");
                return 2;
            }
        }
    }
    let Some(config) = cli_config() else { return 1 };
    send_test::run(&config, &msg)
}

/// Publishes a hand-written envelope — the way to try the W-series
/// extensions end to end before any producer emits them.
fn send_json(source: Option<&str>) -> i32 {
    let text = match source {
        None => {
            eprintln!("usage: newsflash send-json <file.json | ->");
            return 2;
        }
        Some("-") => {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
            s
        }
        Some(path) => match std::fs::read_to_string(PathBuf::from(path)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {path}: {e}");
                return 1;
            }
        },
    };
    if let Err(e) = parse_envelope(text.as_bytes()) {
        eprintln!("not a valid v1 envelope ({e:?}): {}", e.remedy());
        return 2;
    }
    let Some(config) = cli_config() else { return 1 };
    match HubClient::new(&config).publish(&text) {
        Ok(id) => {
            println!("published as message {id} on {}", config.topic);
            0
        }
        Err(e) => {
            eprintln!(
                "publish failed ({}): {}",
                e.status
                    .map(|s| s.to_string())
                    .unwrap_or("transport".into()),
                e.detail
            );
            1
        }
    }
}

/// Used by the demo: whether clicks will reach this process.
pub fn daemon_running() -> bool {
    instance::is_running()
}

pub fn ensure_assets() -> PathBuf {
    let dir = paths::assets_dir();
    extract_assets(&dir);
    dir
}

pub fn stop_requested_flag() -> Arc<AtomicBool> {
    let term = Arc::new(AtomicBool::new(false));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term));
    term
}

pub fn is_set(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}
