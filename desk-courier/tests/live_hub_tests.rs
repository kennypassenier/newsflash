//! Live tests against the REAL mailbox binary (AR18): what no mock can
//! express — lease/redelivery, dead letters, policy effect, the
//! first-poll trap. `#[ignore]` in CI (the remote cannot build the
//! private mailbox repo); run locally via `scripts/drill.sh`.

use courier_core::envelope::parse_from_hub;
use courier_core::toast::Language;
use desk_courier::config::Config;
use desk_courier::hub_client::{HubClient, PolicyOutcome};
use desk_courier::send_test::{TestMessage, build_envelope};
use std::process::{Child, Command};
use std::time::Duration;

struct ScratchHub {
    child: Child,
    url: String,
    #[allow(dead_code)]
    dir: std::path::PathBuf,
}

impl Drop for ScratchHub {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mailbox_bin() -> String {
    std::env::var("MAILBOX_BIN").unwrap_or_else(|_| {
        format!(
            "{}/Projects/mailbox/target/release/mailbox",
            std::env::var("HOME").unwrap()
        )
    })
}

fn start_hub(port: u16) -> ScratchHub {
    let dir = std::env::temp_dir().join(format!("dc-live-{port}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(mailbox_bin())
        .env("MAILBOX_LISTEN", format!("127.0.0.1:{port}"))
        .env("MAILBOX_DATA_DIR", &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("mailbox binary must exist for live tests (MAILBOX_BIN)");
    let url = format!("http://127.0.0.1:{port}");
    for _ in 0..50 {
        if ureq::get(&format!("{url}/healthz")).call().is_ok() {
            return ScratchHub { child, url, dir };
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("scratch hub did not come up on {url}");
}

fn config_for(hub: &ScratchHub) -> Config {
    Config {
        hub_url: hub.url.clone(),
        topic: "notify.kenny".into(),
        subscription: "desktop".into(),
        language: Language::Nl,
        ttl_ms: 600_000,
        sound_file: None,
        token: "unused-open-hub".into(),
    }
}

fn envelope(title: &str) -> String {
    build_envelope(&TestMessage {
        title: title.into(),
        message: format!("live test: {title}"),
        priority: "info".into(),
    })
}

#[test]
#[ignore = "needs the real mailbox binary; run via scripts/drill.sh"]
fn live_s6a_publish_receive_ack_round_trip() {
    let hub = start_hub(18931);
    let client = HubClient::new(&config_for(&hub));

    // The drill-proven cold start: poll first (404 — topic missing),
    // then publish, then poll from=beginning picks it up.
    let first = client.receive(true);
    assert_eq!(first.unwrap_err().status, Some(404));

    let published_id = client.publish(&envelope("Rondje")).unwrap();
    let (message, _) = client.receive(true).unwrap().expect("message due");
    assert_eq!(message.id, published_id);
    let env = parse_from_hub(&message.payload).unwrap();
    assert_eq!(env.title.unwrap().nl.as_deref(), Some("Rondje"));

    let (outcome, _) = client.settle(&message.id, true, false);
    assert_eq!(outcome, courier_core::settle::SettleCallOutcome::Settled);
    // Acked = gone: the next window is empty.
    assert!(client.receive(false).unwrap().is_none());
}

#[test]
#[ignore = "needs the real mailbox binary; run via scripts/drill.sh"]
fn live_k5_ar7_policy_bootstrap_is_idempotent_and_effective() {
    let hub = start_hub(18932);
    let client = HubClient::new(&config_for(&hub));

    client.publish(&envelope("Policy")).unwrap();
    let _ = client.receive(true).unwrap(); // subscription now exists

    let first = client.ensure_policy(600_000).unwrap();
    assert!(matches!(first, PolicyOutcome::Written { .. }));
    let second = client.ensure_policy(600_000).unwrap();
    assert!(
        matches!(second, PolicyOutcome::AlreadyRight),
        "bootstrap must be idempotent"
    );

    // Read back what the hub now enforces (standing rule 13a).
    let body: serde_json::Value = ureq::get(&format!(
        "{}/api/t/notify.kenny/subs/desktop/policy",
        hub.url
    ))
    .call()
    .unwrap()
    .into_string()
    .map(|t| serde_json::from_str(&t).unwrap())
    .unwrap();
    assert_eq!(body["effective"]["ttl_ms"].as_u64(), Some(600_000));
    assert!(
        body["explicit"]["lease_ms"].is_null(),
        "lease must track hub defaults"
    );
}

#[test]
#[ignore = "needs the real mailbox binary; run via scripts/drill.sh"]
fn live_k3_an_unacked_message_is_redelivered_with_a_higher_attempt() {
    let hub = start_hub(18933);
    let client = HubClient::new(&config_for(&hub));

    client.publish(&envelope("Nack")).unwrap();
    let (message, _) = client.receive(true).unwrap().expect("message due");
    assert_eq!(message.attempt, 1);
    let (outcome, _) = client.settle(&message.id, false, false); // nack
    assert_eq!(outcome, courier_core::settle::SettleCallOutcome::Settled);

    // Redelivery honors the backoff (hub default 1s per attempt).
    let mut redelivered = None;
    for _ in 0..10 {
        if let Some((m, _)) = client.receive(false).unwrap() {
            redelivered = Some(m);
            break;
        }
    }
    let redelivered = redelivered.expect("nacked message must come back");
    assert_eq!(redelivered.id, message.id);
    assert!(
        redelivered.attempt >= 2,
        "attempt was {}",
        redelivered.attempt
    );
    let _ = client.settle(&redelivered.id, true, false);
}

#[test]
#[ignore = "needs the real mailbox binary; run via scripts/drill.sh"]
fn live_m9_poison_lands_visibly_in_the_dead_letters() {
    let hub = start_hub(18934);
    let client = HubClient::new(&config_for(&hub));

    // Raw non-envelope publish (the dashboard-test-box scenario).
    ureq::post(&format!("{}/t/notify.kenny", hub.url))
        .set("content-type", "text/plain")
        .send_string("hello, not an envelope")
        .unwrap();
    let (message, _) = client.receive(true).unwrap().expect("message due");
    let parse = parse_from_hub(&message.payload);
    assert!(parse.is_err(), "plain text must not parse as an envelope");

    let (outcome, _) = client.settle(&message.id, false, true); // poison
    assert_eq!(outcome, courier_core::settle::SettleCallOutcome::Settled);

    let dead: serde_json::Value =
        ureq::get(&format!("{}/api/t/notify.kenny/subs/desktop/dead", hub.url))
            .call()
            .unwrap()
            .into_string()
            .map(|t| serde_json::from_str(&t).unwrap())
            .unwrap();
    let listed = dead.to_string();
    assert!(
        listed.contains(&message.id),
        "dead letter list must show the poisoned id: {listed}"
    );
}

#[test]
#[ignore = "needs the real mailbox binary; run via scripts/drill.sh"]
fn live_ar21_unarchive_on_a_healthy_subscription_is_a_safe_noop() {
    let hub = start_hub(18935);
    let client = HubClient::new(&config_for(&hub));
    client.publish(&envelope("Archief")).unwrap();
    let _ = client.receive(true).unwrap();
    // A real 30-day archive cannot be simulated; prove the endpoint the
    // AR21 path calls answers, and that calling it on a live
    // subscription changes nothing (the hub's documented behaviour).
    client.unarchive().expect("unarchive endpoint reachable");
    client.publish(&envelope("Na archief")).unwrap();
    let (m, _) = client.receive(false).unwrap().expect("still receiving");
    let _ = client.settle(&m.id, true, false);
}
