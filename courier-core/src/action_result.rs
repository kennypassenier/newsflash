//! M10 reply path (pipeline-v2 K12 amendment, 2026-08-30): builds the
//! `action_result` envelope newsflash publishes to `notify.actions`
//! after a toast button is clicked. Pure and testable — the shell owns
//! the id generation input (clock, pid) and the actual publish.
//!
//! Shape (no formal schema from pipeline-v2 beyond prose, so this is
//! newsflash's own minimal, documented choice, mirroring how the v1
//! envelope itself carries kind-specific fields under `data`):
//! `{"v":1,"id":<fresh>,"kind":"action_result","source":"newsflash",
//!   "ack_id":<original ack_id, if any>,
//!   "data":{"original_envelope_id":<original env.id>,"action_id":<clicked id>}}`
//!
//! Windows port (feat-win-9): when the toast carried inputs, their values ride
//! along as `data.inputs` (`{"<input id>": "<value>"}`) — additive, and
//! omitted entirely when empty, so the Linux shape is byte-identical.

/// The contract topic (pinned, not configurable — standing rule 27:
/// contract values stay pinned constants).
pub const ACTIONS_TOPIC: &str = "notify.actions";

pub fn build_action_result(
    fresh_id: &str,
    original_envelope_id: &str,
    ack_id: Option<&str>,
    action_id: &str,
    inputs: &[(String, String)],
) -> String {
    let mut body = serde_json::json!({
        "v": 1,
        "id": fresh_id,
        "kind": "action_result",
        "source": "newsflash",
        "data": {
            "original_envelope_id": original_envelope_id,
            "action_id": action_id,
        }
    });
    if let Some(ack) = ack_id {
        body["ack_id"] = serde_json::Value::String(ack.to_string());
    }
    if !inputs.is_empty() {
        let map: serde_json::Map<String, serde_json::Value> = inputs
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        body["data"]["inputs"] = serde_json::Value::Object(map);
    }
    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k12_the_shape_carries_original_id_and_chosen_action() {
        let json = build_action_result("fresh-1", "orig-42", Some("ack-7"), "snooze", &[]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["id"], "fresh-1");
        assert_eq!(v["kind"], "action_result");
        assert_eq!(v["source"], "newsflash");
        assert_eq!(v["ack_id"], "ack-7");
        assert_eq!(v["data"]["original_envelope_id"], "orig-42");
        assert_eq!(v["data"]["action_id"], "snooze");
    }

    #[test]
    fn a_missing_ack_id_is_omitted_not_null() {
        let json = build_action_result("fresh-1", "orig-42", None, "gelezen", &[]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("ack_id").is_none());
        assert!(v["data"].get("inputs").is_none());
    }

    #[test]
    fn feat_win_9_input_values_ride_along_under_data_inputs() {
        let inputs = vec![
            ("reply".to_string(), "on my way".to_string()),
            ("snooze_minutes".to_string(), "60".to_string()),
        ];
        let json = build_action_result("fresh-1", "orig-42", None, "send", &inputs);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["data"]["inputs"]["reply"], "on my way");
        assert_eq!(v["data"]["inputs"]["snooze_minutes"], "60");
        assert_eq!(v["data"]["action_id"], "send");
    }
}
