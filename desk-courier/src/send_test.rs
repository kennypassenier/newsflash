//! M8 · `desk-courier send-test`: publish one valid v1 test envelope —
//! the interim producer (SCOPE S11d) and the drill tool.

use crate::config::Config;
use crate::hub_client::HubClient;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TestMessage {
    pub title: String,
    pub message: String,
    pub priority: String,
}

pub fn build_envelope(msg: &TestMessage) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let id = format!("test-{}-{}", millis, std::process::id());
    serde_json::json!({
        "v": 1,
        "id": id,
        "source": "desk-courier",
        "kind": "notification",
        "audience": "kenny",
        "priority": msg.priority,
        "title": { "nl": msg.title, "en": msg.title },
        "message": { "nl": msg.message, "en": msg.message },
    })
    .to_string()
}

pub fn run(config: &Config, msg: &TestMessage) -> i32 {
    // The drill tool must not mislead a drill (hardening gap G11): a
    // typo'd priority silently rendering as info would read as "critical
    // toasts are broken".
    if !["info", "warning", "critical"].contains(&msg.priority.as_str()) {
        eprintln!(
            "priority {:?} is not a v1 priority. Use info, warning or critical.",
            msg.priority
        );
        return 2;
    }
    let client = HubClient::new(config);
    match client.publish(&build_envelope(msg)) {
        Ok(id) => {
            println!(
                "published test envelope as message {id} on {}",
                config.topic
            );
            0
        }
        Err(e) => {
            eprintln!(
                "publish failed ({}): {}. Is the hub reachable and the token valid?",
                e.status
                    .map(|s| s.to_string())
                    .unwrap_or("transport".into()),
                e.detail
            );
            1
        }
    }
}
