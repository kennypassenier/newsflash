//! The hub side of the wire (pure parsing): the `envelope=json`
//! receive response and the status classification the loop acts on.
//! The response shape is pinned by a vector captured from the real
//! mailbox binary (AR18) — see `tests/vectors/`.

use serde::Deserialize;

/// One payload key is always present, and which one says how the hub
/// understood the bytes (mailbox G8: never a silent transformation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubPayload {
    Json(serde_json::Value),
    Text(String),
    /// base64 — binary can never be an envelope; the settle table
    /// poisons it without decoding.
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubMessage {
    /// The HUB message id — the settle/dedup key (AR5), and the id
    /// every ack/nack names. Not the payload envelope's `id`.
    pub id: String,
    pub attempt: u32,
    pub published_at_ms: u64,
    pub payload: HubPayload,
}

#[derive(Debug, Deserialize)]
struct RawHubResponse {
    id: String,
    #[serde(default)]
    attempt: u32,
    published_at: u64,
    #[serde(default)]
    payload: Option<serde_json::Value>,
    #[serde(default)]
    payload_text: Option<String>,
    #[serde(default)]
    payload_base64: Option<String>,
}

/// A response we cannot read is a hub-contract problem, not a message
/// problem: there is no trustworthy id to nack, so the shell logs it
/// and lets the lease expire (redelivery will retry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubResponseError(pub String);

pub fn parse_hub_response(body: &[u8]) -> Result<HubMessage, HubResponseError> {
    let raw: RawHubResponse =
        serde_json::from_slice(body).map_err(|e| HubResponseError(e.to_string()))?;
    if raw.id.trim().is_empty() {
        return Err(HubResponseError("hub response carries an empty id".into()));
    }
    let payload = if let Some(v) = raw.payload {
        HubPayload::Json(v)
    } else if let Some(t) = raw.payload_text {
        HubPayload::Text(t)
    } else if raw.payload_base64.is_some() {
        HubPayload::Binary
    } else {
        return Err(HubResponseError(
            "hub response carries no payload key at all".into(),
        ));
    };
    Ok(HubMessage {
        id: raw.id,
        attempt: raw.attempt,
        published_at_ms: raw.published_at,
        payload,
    })
}

/// Client-side TTL check (AR5): the hub can only expire *unclaimed*
/// messages, so a message claimed just before a suspend must not be
/// rendered hours later. Mirrors the hub's own `is_past_ttl`.
pub fn is_stale(published_at_ms: u64, ttl_ms: u64, now_ms: u64) -> bool {
    published_at_ms.saturating_add(ttl_ms) <= now_ms
}

/// The three failure classes the loop never merges (AR9): a revoked
/// token must not impersonate "hub down", and an archived subscription
/// must not impersonate either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubErrorClass {
    /// Transport error or 5xx — "hub down", plain backoff.
    Unreachable,
    /// 401/403 — backoff too, but its own state and remedy log line.
    Auth,
    /// 409 on the receive path — archived subscription (AR21).
    Archived,
    /// Anything else 4xx — log and back off; a client bug, not a hub outage.
    Other,
}

pub fn classify_receive_status(status: Option<u16>) -> HubErrorClass {
    match status {
        None => HubErrorClass::Unreachable,
        Some(s) if s >= 500 => HubErrorClass::Unreachable,
        Some(401) | Some(403) => HubErrorClass::Auth,
        Some(409) => HubErrorClass::Archived,
        Some(_) => HubErrorClass::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_payload_response_parses() {
        let msg = parse_hub_response(
            br#"{"id":"41","topic":"notify.kenny","attempt":1,"published_at":1756400000000,"content_type":"application/json","payload":{"v":1}}"#,
        )
        .unwrap();
        assert_eq!(msg.id, "41");
        assert_eq!(msg.attempt, 1);
        assert_eq!(msg.published_at_ms, 1_756_400_000_000);
        assert!(matches!(msg.payload, HubPayload::Json(_)));
    }

    #[test]
    fn text_and_base64_payloads_are_distinguished() {
        let text = parse_hub_response(br#"{"id":"1","published_at":5,"payload_text":"hi"}"#);
        assert_eq!(text.unwrap().payload, HubPayload::Text("hi".into()));
        let bin = parse_hub_response(br#"{"id":"1","published_at":5,"payload_base64":"AAEC"}"#);
        assert_eq!(bin.unwrap().payload, HubPayload::Binary);
    }

    #[test]
    fn a_payloadless_or_idless_response_is_a_contract_error() {
        assert!(parse_hub_response(br#"{"id":"1","published_at":5}"#).is_err());
        assert!(parse_hub_response(br#"{"id":" ","published_at":5,"payload_text":"x"}"#).is_err());
        assert!(parse_hub_response(b"nope").is_err());
    }

    #[test]
    fn ar5_staleness_mirrors_the_hub_ttl_rule() {
        assert!(!is_stale(1_000, 600_000, 600_999));
        assert!(is_stale(1_000, 600_000, 601_000));
        assert!(is_stale(0, 0, 0));
    }

    #[test]
    fn ar9_the_three_failure_classes_never_merge() {
        assert_eq!(classify_receive_status(None), HubErrorClass::Unreachable);
        assert_eq!(
            classify_receive_status(Some(503)),
            HubErrorClass::Unreachable
        );
        assert_eq!(classify_receive_status(Some(401)), HubErrorClass::Auth);
        assert_eq!(classify_receive_status(Some(403)), HubErrorClass::Auth);
        assert_eq!(classify_receive_status(Some(409)), HubErrorClass::Archived);
        assert_eq!(classify_receive_status(Some(404)), HubErrorClass::Other);
    }
}
