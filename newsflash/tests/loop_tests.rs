//! Hardening gap G3: the run loop itself, exercised as the REAL binary
//! against a scripted mock hub, with PATH shims for the subprocesses
//! and the journal (stderr) asserted. Closes the loop-level promises of
//! AR7 (re-assert on reconnect), AR9 (named states), AR21 (archived →
//! unarchive → resume), AR22 (hold, don't consume), M4 (SIGTERM
//! settles), M11 (log lines), K4/S6b (one render across a kill), K9
//! (token never in loop output).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TOKEN: &str = "LOOP-SECRET-TOKEN-77x";

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    url: String,
}

struct MockHub {
    addr: String,
    requests: Arc<Mutex<Vec<Seen>>>,
}

/// Scripted hub: /next answers pop from the queue, then 204 (held
/// ~150 ms so the loop cannot spin hot); policy/ack/nack/unarchive
/// answer sensibly and everything is logged.
fn scripted_hub(polls: Vec<(u16, String)>) -> MockHub {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    let queue = Mutex::new(VecDeque::from(polls));
    std::thread::spawn(move || {
        loop {
            let Ok(request) = server.recv() else { return };
            let method = request.method().to_string();
            let url = request.url().to_string();
            log.lock().unwrap().push(Seen {
                method: method.clone(),
                url: url.clone(),
            });
            let (status, body): (u16, String) = if url.contains("/next") {
                match queue.lock().unwrap().pop_front() {
                    Some(scripted) => scripted,
                    None => {
                        std::thread::sleep(Duration::from_millis(150));
                        (204, String::new())
                    }
                }
            } else if url.ends_with("/policy") && method == "GET" {
                (
                    200,
                    r#"{"effective":{"ttl_ms":600000},"explicit":{"lease_ms":null,"max_attempts":null,"backoff_ms":null,"ttl_ms":600000}}"#.to_string(),
                )
            } else {
                (200, "{}".to_string())
            };
            let _ =
                request.respond(tiny_http::Response::from_string(body).with_status_code(status));
        }
    });
    MockHub { addr, requests }
}

fn envelope_poll(hub_id: &str, title: &str) -> (u16, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    (
        200,
        format!(
            r#"{{"id":"{hub_id}","topic":"notify.kenny","attempt":1,"published_at":{now},"content_type":"application/json","payload":{{"v":1,"id":"p-{hub_id}","title":{{"nl":"{title}"}}}}}}"#
        ),
    )
}

struct TestEnv {
    dir: PathBuf,
    shim_log: PathBuf,
    fail_notify: PathBuf,
    fail_busctl: PathBuf,
}

fn test_env(name: &str) -> TestEnv {
    let dir = std::env::temp_dir().join(format!("dc-loop-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let shim_log = dir.join("shim.log");
    let fail_notify = dir.join("fail-notify");
    let fail_busctl = dir.join("fail-busctl");
    for (name, flag) in [
        ("notify-send", &fail_notify),
        ("paplay", &fail_notify),
        ("busctl", &fail_busctl),
    ] {
        let path = dir.join("bin").join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"{name} $*\" >> {}\n[ -e {} ] && exit 1\nexit 0\n",
                shim_log.display(),
                flag.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    TestEnv {
        dir,
        shim_log,
        fail_notify,
        fail_busctl,
    }
}

impl TestEnv {
    fn journal(&self) -> PathBuf {
        self.dir.join("journal.log")
    }

    fn spawn_courier(&self, hub_url: &str) -> Child {
        let config = self.dir.join("config.toml");
        std::fs::write(&config, format!("hub_url = \"{hub_url}\"\n")).unwrap();
        let journal = std::fs::File::create(self.journal()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_newsflash"))
            .arg("--config")
            .arg(&config)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("XDG_STATE_HOME", self.dir.join("state"))
            .env("KYU_TOKEN", TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::from(journal))
            .spawn()
            .unwrap()
    }

    fn read_journal(&self) -> String {
        std::fs::read_to_string(self.journal()).unwrap_or_default()
    }

    fn read_shim_log(&self) -> String {
        std::fs::read_to_string(&self.shim_log).unwrap_or_default()
    }
}

fn wait_until(what: &str, deadline_secs: u64, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(deadline_secs);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn sigterm(child: &Child) {
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
}

fn wait_exit(child: &mut Child, secs: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("courier did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn urls(requests: &Arc<Mutex<Vec<Seen>>>) -> Vec<String> {
    requests
        .lock()
        .unwrap()
        .iter()
        .map(|s| format!("{} {}", s.method, s.url))
        .collect()
}

#[test]
fn ar21_archived_flow_unarchives_resumes_and_renders() {
    let env = test_env("archived");
    let hub = scripted_hub(vec![
        (
            409,
            r#"{"error":"subscription \"desktop\" on topic \"notify.kenny\" is archived","remedy":"unarchive it"}"#.to_string(),
        ),
        envelope_poll("hub-arch-1", "Na de winterslaap"),
    ]);
    let mut child = env.spawn_courier(&hub.addr);

    wait_until("render after unarchive", 15, || {
        env.read_shim_log().contains("Na de winterslaap")
    });
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));

    let calls = urls(&hub.requests);
    let unarchive_at = calls
        .iter()
        .position(|u| u.contains("/unarchive"))
        .expect("unarchive must be called");
    // The poll after the unarchive replays from the beginning (AR7/AR21).
    let next_poll = calls[unarchive_at..]
        .iter()
        .find(|u| u.contains("/next"))
        .expect("polling resumes");
    assert!(next_poll.contains("from=beginning"), "{next_poll}");
    assert!(calls.iter().any(|u| u.contains("/ack/hub-arch-1")));

    let journal = env.read_journal();
    assert!(journal.contains("archived"), "{journal}");
    assert!(journal.contains("unarchived; resuming"), "{journal}");
    assert!(journal.contains("rendered hub-arch-1"), "{journal}");
}

#[test]
fn ar9_k9_auth_rejection_is_named_with_remedy_and_leaks_no_token() {
    let env = test_env("auth");
    let hub = scripted_hub(vec![(
        401,
        r#"{"error":"no or wrong token","remedy":"authorization: Bearer <token>"}"#.to_string(),
    )]);
    let mut child = env.spawn_courier(&hub.addr);

    wait_until("auth-rejected line", 15, || {
        env.read_journal().contains("hub rejected the token")
    });
    // Recovery: the next poll (after ~2 s backoff) answers 204; the
    // courier reconnects and asserts the policy (AR7).
    wait_until("policy assert after recovery", 15, || {
        urls(&hub.requests)
            .iter()
            .any(|u| u.contains("/policy") && u.starts_with("GET"))
    });
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));

    let journal = env.read_journal();
    assert!(journal.contains("re-mint"), "remedy missing: {journal}");
    assert!(journal.contains("hub reachable"), "{journal}");
    assert!(
        !journal.contains(TOKEN),
        "token leaked into journal: {journal}"
    );
}

#[test]
fn ar22_no_polling_while_the_daemon_is_absent() {
    let env = test_env("hold");
    std::fs::write(&env.fail_busctl, "").unwrap(); // daemon "absent"
    let hub = scripted_hub(vec![]);
    let mut child = env.spawn_courier(&hub.addr);

    wait_until("hold line", 15, || {
        env.read_journal().contains("holding, not consuming")
    });
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !urls(&hub.requests).iter().any(|u| u.contains("/next")),
        "the courier polled while the daemon was absent"
    );

    std::fs::remove_file(&env.fail_busctl).unwrap(); // daemon "returns"
    wait_until("polling resumes", 15, || {
        urls(&hub.requests).iter().any(|u| u.contains("/next"))
    });
    assert!(env.read_journal().contains("notification daemon present"));
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));
}

#[test]
fn k4_s6b_exactly_one_render_across_a_hard_kill_and_redelivery() {
    let env = test_env("dedup");
    // Run 1 renders and acks hub-dup-1.
    let hub1 = scripted_hub(vec![envelope_poll("hub-dup-1", "Eenmalig")]);
    let mut child = env.spawn_courier(&hub1.addr);
    wait_until("first render acked", 15, || {
        urls(&hub1.requests)
            .iter()
            .any(|u| u.contains("/ack/hub-dup-1"))
    });
    // Hard kill: no shutdown path, the persisted store is what survives.
    let _ = child.kill();
    let _ = child.wait();

    // Run 2: the hub redelivers the same hub id (at-least-once).
    let hub2 = scripted_hub(vec![envelope_poll("hub-dup-1", "Eenmalig")]);
    let mut child = env.spawn_courier(&hub2.addr);
    wait_until("redelivery acked silently", 15, || {
        urls(&hub2.requests)
            .iter()
            .any(|u| u.contains("/ack/hub-dup-1"))
    });
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));

    let renders = env
        .read_shim_log()
        .lines()
        .filter(|l| l.contains("Eenmalig"))
        .count();
    assert_eq!(renders, 1, "S6b: exactly one toast across the kill");
    assert!(env.read_journal().contains("redelivery of a seen id"));
}

#[test]
fn k3_ar22_render_failure_nacks_and_reprobes() {
    let env = test_env("renderfail");
    std::fs::write(&env.fail_notify, "").unwrap(); // renderer broken
    let hub = scripted_hub(vec![envelope_poll("hub-rf-1", "Mislukt")]);
    let mut child = env.spawn_courier(&hub.addr);

    wait_until("nack after render failure", 15, || {
        urls(&hub.requests)
            .iter()
            .any(|u| u.contains("/nack/hub-rf-1"))
    });
    let nack = urls(&hub.requests)
        .into_iter()
        .find(|u| u.contains("/nack/"))
        .unwrap();
    assert!(
        !nack.contains("dead=true"),
        "a render failure is transient, not poison: {nack}"
    );
    wait_until("re-probe after failure", 15, || {
        env.read_shim_log()
            .lines()
            .filter(|l| l.contains("busctl"))
            .count()
            >= 2
    });
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));
    assert!(env.read_journal().contains("render failed"));
}

#[test]
fn m4_m11_sigterm_settles_and_the_journal_carries_the_lifecycle() {
    let env = test_env("sigterm");
    let hub = scripted_hub(vec![]);
    let mut child = env.spawn_courier(&hub.addr);
    wait_until("startup + first poll", 15, || {
        urls(&hub.requests).iter().any(|u| u.contains("/next"))
    });
    sigterm(&child);
    assert_eq!(wait_exit(&mut child, 10), Some(0));

    let journal = env.read_journal();
    for expected in [
        "newsflash 0.1.0 starting",
        "notification daemon present",
        "hub reachable",
        "shutdown: in-flight work settled, dedup store persisted",
    ] {
        assert!(
            journal.contains(expected),
            "missing {expected:?}: {journal}"
        );
    }
    assert!(!journal.contains(TOKEN));
    // The startup summary names the config in force.
    assert!(journal.contains("ttl=10min") && journal.contains("sound=off"));
}
