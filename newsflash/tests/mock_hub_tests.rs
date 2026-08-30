//! K1/K3/K5/K9 against a mock hub. The mock's envelope body is the
//! vector captured from the REAL kyu binary during the L2 drill
//! (AR18) — the mock serves evidence, not fiction. What the mock
//! cannot express (real lease/redelivery/TTL/archive semantics) is
//! covered by the live tests in `live_hub_tests.rs`.

use courier_core::hub::HubPayload;
use courier_core::toast::Language;
use newsflash::config::Config;
use newsflash::hub_client::{HubClient, PolicyOutcome};
use std::sync::{Arc, Mutex};

const REAL_ENVELOPE_RESPONSE: &str =
    include_str!("../../courier-core/tests/vectors/hub_envelope_response.json");

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    url: String,
    auth: Option<String>,
    body: String,
}

/// One-shot mock hub: serves the scripted responses in order, records
/// what it saw.
fn mock_hub(responses: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_thread = Arc::clone(&seen);
    std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok(mut request) = server.recv() else {
                return;
            };
            let mut req_body = String::new();
            let _ = std::io::Read::read_to_string(request.as_reader(), &mut req_body);
            seen_thread.lock().unwrap().push(Seen {
                method: request.method().to_string(),
                url: request.url().to_string(),
                auth: request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("authorization"))
                    .map(|h| h.value.to_string()),
                body: req_body,
            });
            let response = tiny_http::Response::from_string(body).with_status_code(status);
            let _ = request.respond(response);
        }
    });
    (addr, seen)
}

fn config_for(hub_url: &str) -> Config {
    Config {
        hub_url: hub_url.to_string(),
        topic: "notify.kenny".into(),
        subscription: "desktop".into(),
        language: Language::Nl,
        ttl_ms: 600_000,
        sound_file: None,
        token: "mock-token-XYZ".into(),
        interactive_wait_margin_ms: 30_000,
    }
}

#[test]
fn k1_receive_sends_the_right_request_and_parses_the_real_vector() {
    let (addr, seen) = mock_hub(vec![(200, REAL_ENVELOPE_RESPONSE)]);
    let client = HubClient::new(&config_for(&addr));
    let (message, _notice) = client.receive(false).unwrap().unwrap();

    assert_eq!(message.id, "01M1579EQ12GJG6QKY5478K8TT");
    assert_eq!(message.attempt, 1);
    assert!(matches!(message.payload, HubPayload::Json(_)));
    // The payload envelope parses end-to-end from the captured bytes.
    let env = courier_core::envelope::parse_from_hub(&message.payload).unwrap();
    assert_eq!(env.title.unwrap().nl.as_deref(), Some("Vriezer"));

    let saw = seen.lock().unwrap()[0].clone();
    assert_eq!(saw.method, "GET");
    assert!(saw.url.starts_with("/t/notify.kenny/next?"), "{}", saw.url);
    for expected in ["as=desktop", "envelope=json", "wait=20"] {
        assert!(saw.url.contains(expected), "{}", saw.url);
    }
    assert!(!saw.url.contains("from=beginning"));
    assert_eq!(saw.auth.as_deref(), Some("Bearer mock-token-XYZ"));
}

#[test]
fn ar7_the_first_poll_of_a_run_replays_from_the_beginning() {
    let (addr, seen) = mock_hub(vec![(204, "")]);
    let client = HubClient::new(&config_for(&addr));
    assert!(client.receive(true).unwrap().is_none()); // 204 = empty window
    assert!(seen.lock().unwrap()[0].url.contains("from=beginning"));
}

#[test]
fn k3_ack_and_nack_hit_the_right_endpoints() {
    let (addr, seen) = mock_hub(vec![(200, "{}"), (200, "{}"), (200, "{}")]);
    let client = HubClient::new(&config_for(&addr));
    client.settle("m-1", true, false);
    client.settle("m-2", false, false);
    client.settle("m-3", false, true);
    let saw = seen.lock().unwrap();
    assert!(saw[0].url.contains("/t/notify.kenny/ack/m-1?as=desktop"));
    assert!(saw[1].url.contains("/t/notify.kenny/nack/m-2?as=desktop"));
    assert!(
        saw[2]
            .url
            .contains("/t/notify.kenny/nack/m-3?as=desktop&dead=true")
    );
    assert!(saw.iter().all(|s| s.method == "POST"));
}

#[test]
fn k5_ar7_policy_put_carries_exactly_the_one_owned_field() {
    // GET: nothing explicit → PUT expected.
    let (addr, seen) = mock_hub(vec![
        (
            200,
            r#"{"effective":{"ttl_ms":null},"explicit":{"lease_ms":null,"max_attempts":null,"backoff_ms":null,"ttl_ms":null}}"#,
        ),
        (
            200,
            r#"{"effective":{"ttl_ms":600000},"explicit":{"ttl_ms":600000}}"#,
        ),
    ]);
    let client = HubClient::new(&config_for(&addr));
    let outcome = client.ensure_policy(600_000).unwrap();
    assert!(matches!(
        outcome,
        PolicyOutcome::Written {
            overrode_explicit: false
        }
    ));
    let saw = seen.lock().unwrap();
    assert_eq!(saw[1].method, "PUT");
    let body: serde_json::Value = serde_json::from_str(&saw[1].body).unwrap();
    assert_eq!(body, serde_json::json!({"ttl_ms": 600000}));
}

#[test]
fn ar7_a_policy_already_right_is_left_alone() {
    let (addr, seen) = mock_hub(vec![(
        200,
        r#"{"effective":{"ttl_ms":600000},"explicit":{"lease_ms":null,"max_attempts":null,"backoff_ms":null,"ttl_ms":600000}}"#,
    )]);
    let client = HubClient::new(&config_for(&addr));
    assert!(matches!(
        client.ensure_policy(600_000).unwrap(),
        PolicyOutcome::AlreadyRight
    ));
    assert_eq!(seen.lock().unwrap().len(), 1); // GET only, no PUT
}

#[test]
fn ar7_a_human_override_is_replaced_and_reported() {
    let (addr, _seen) = mock_hub(vec![
        (
            200,
            r#"{"effective":{},"explicit":{"lease_ms":90000,"max_attempts":null,"backoff_ms":null,"ttl_ms":600000}}"#,
        ),
        (200, "{}"),
    ]);
    let client = HubClient::new(&config_for(&addr));
    assert!(matches!(
        client.ensure_policy(600_000).unwrap(),
        PolicyOutcome::Written {
            overrode_explicit: true
        }
    ));
}

#[test]
fn k9_the_token_reaches_the_wire_but_never_the_output() {
    // The real binary, one-shot via send-test, against a mock that
    // answers 201. Proves: header on the wire, token in no output.
    let (addr, seen) = mock_hub(vec![(201, r#"{"id":"pub-1"}"#)]);
    let dir = std::env::temp_dir().join(format!("dc-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, format!("hub_url = \"{addr}\"\n")).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_newsflash"))
        .args(["send-test", "--config"])
        .arg(&cfg)
        .args(["--title", "Scan", "--message", "scan test"])
        .env("KYU_TOKEN", "SECRET-SCAN-TOKEN-9f3")
        .output()
        .unwrap();

    let all_output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{all_output}");
    assert!(
        !all_output.contains("SECRET-SCAN-TOKEN-9f3"),
        "token leaked into output: {all_output}"
    );
    let saw = seen.lock().unwrap()[0].clone();
    assert_eq!(saw.auth.as_deref(), Some("Bearer SECRET-SCAN-TOKEN-9f3"));
    // And the published body is a valid v1 envelope.
    let env = courier_core::envelope::parse_envelope(saw.body.as_bytes()).unwrap();
    assert_eq!(env.title.unwrap().nl.as_deref(), Some("Scan"));
}
