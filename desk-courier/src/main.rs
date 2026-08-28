//! desk-courier — renders messages from the mailbox hub's
//! `notify.kenny` topic as desktop toasts. See docs/SCOPE.md.

use desk_courier::{config, logx, run, send_test};
use std::path::PathBuf;

const USAGE: &str = "usage:
  desk-courier [--config <path>]                    run the courier
  desk-courier send-test [--config <path>]
               [--title T] [--message M] [--priority info|warning|critical]
  desk-courier --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("desk-courier {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }

    let mut config_path: Option<PathBuf> = None;
    let mut send: Option<send_test::TestMessage> = None;
    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "send-test" => {
                send = Some(send_test::TestMessage {
                    title: "Testbericht".into(),
                    message: "desk-courier send-test".into(),
                    priority: "info".into(),
                });
            }
            "--config" => config_path = it.next().map(PathBuf::from),
            "--title" => {
                if let (Some(s), Some(v)) = (send.as_mut(), it.next()) {
                    s.title = v.clone();
                }
            }
            "--message" => {
                if let (Some(s), Some(v)) = (send.as_mut(), it.next()) {
                    s.message = v.clone();
                }
            }
            "--priority" => {
                if let (Some(s), Some(v)) = (send.as_mut(), it.next()) {
                    s.priority = v.clone();
                }
            }
            other => {
                eprintln!("unknown argument {other:?}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let path = config_path.unwrap_or_else(config::default_path);
    let config = match config::load(&path) {
        Ok(c) => c,
        Err(remedy) => {
            // The one exiting error class (AR8): fatal-config, with the
            // remedy as the last thing in the journal.
            logx::error(&remedy);
            std::process::exit(1);
        }
    };

    let code = match send {
        Some(msg) => send_test::run(&config, &msg),
        None => run::run(config),
    };
    std::process::exit(code);
}
