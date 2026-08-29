//! Envelope v1 DRAFT (study §7) — tolerant reader (AR4): unknown
//! fields are ignored so pipeline-v2 can evolve the schema without
//! breaking us; hard requirements are `v == 1`, a non-empty id and at
//! least one renderable text. A schema change upstream is a mini-round
//! trigger (SCOPE S7), not something to paper over here.

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LocalizedText {
    #[serde(default)]
    pub nl: Option<String>,
    #[serde(default)]
    pub en: Option<String>,
}

impl LocalizedText {
    pub fn is_empty(&self) -> bool {
        !has_text(&self.nl) && !has_text(&self.en)
    }
}

fn has_text(field: &Option<String>) -> bool {
    field.as_deref().is_some_and(|s| !s.trim().is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub ts: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub title: Option<LocalizedText>,
    #[serde(default)]
    pub message: Option<LocalizedText>,
    // tts, ack_id, click_url and data are deliberately not modeled:
    // speech is the DLNA channel's job, click_url is feature M10
    // (Later), and the courier never inspects data (no routing, S9).
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Not JSON, or JSON that misses the required shape entirely.
    Malformed(String),
    /// No `v` field at all — almost certainly a non-envelope publish
    /// (the hub dashboard's test box, W9); its own variant so the dead
    /// letter says so (critic on AR4).
    MissingVersion,
    /// `v` present but not the version this courier speaks.
    UnsupportedVersion(u32),
    /// An envelope without an id cannot be traced.
    MissingId,
    /// Parseable, but no title and no message in any language.
    NothingToRender,
    /// Payload over the AR4 size budget — poisoned without parsing.
    TooLarge(usize),
    /// base64/binary payload — an envelope is never binary.
    BinaryPayload,
}

impl EnvelopeError {
    /// Remedy text (standing rule 11) — shown in logs next to the
    /// poison-pill nack so the dead letter is diagnosable on sight.
    pub fn remedy(&self) -> &'static str {
        match self {
            EnvelopeError::Malformed(_) => {
                "publish a JSON envelope per the v1 draft (study §7); see the dead letter's payload"
            }
            EnvelopeError::MissingVersion => {
                "no v field — a hand-typed test publish? Use newsflash send-test, or add \"v\":1"
            }
            EnvelopeError::UnsupportedVersion(_) => {
                "this courier speaks envelope v1 only; a new version needs a mini-round (SCOPE S7)"
            }
            EnvelopeError::MissingId => "set a non-empty id (ULID or HA context id)",
            EnvelopeError::NothingToRender => {
                "set title and/or message with at least one of nl/en non-empty"
            }
            EnvelopeError::TooLarge(_) => {
                "a toast payload never needs more than 256 KiB; trim the data field"
            }
            EnvelopeError::BinaryPayload => {
                "publish the envelope as JSON (content-type application/json), not binary"
            }
        }
    }
}

pub const ENVELOPE_VERSION: u32 = 1;

/// AR4 size budget: past this, poison without parsing.
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

pub fn parse_envelope(payload: &[u8]) -> Result<Envelope, EnvelopeError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(EnvelopeError::TooLarge(payload.len()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|e| EnvelopeError::Malformed(e.to_string()))?;
    parse_envelope_value(&value)
}

/// Entry point for the hub's `payload` key (already-parsed JSON).
pub fn parse_envelope_value(value: &serde_json::Value) -> Result<Envelope, EnvelopeError> {
    if value.get("v").is_none() {
        return Err(EnvelopeError::MissingVersion);
    }
    let env: Envelope = serde_json::from_value(value.clone())
        .map_err(|e| EnvelopeError::Malformed(e.to_string()))?;
    if env.v != ENVELOPE_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(env.v));
    }
    if env.id.trim().is_empty() {
        return Err(EnvelopeError::MissingId);
    }
    let renderable = env.title.as_ref().is_some_and(|t| !t.is_empty())
        || env.message.as_ref().is_some_and(|m| !m.is_empty());
    if !renderable {
        return Err(EnvelopeError::NothingToRender);
    }
    Ok(env)
}

/// The one funnel from a hub payload to an envelope (AR4/AR5):
/// binary is poison, JSON payloads are size-budgeted via their
/// serialized form, text payloads via their length.
pub fn parse_from_hub(payload: &crate::hub::HubPayload) -> Result<Envelope, EnvelopeError> {
    use crate::hub::HubPayload;
    match payload {
        HubPayload::Binary => Err(EnvelopeError::BinaryPayload),
        HubPayload::Text(t) => parse_envelope(t.as_bytes()),
        HubPayload::Json(v) => {
            let approx = serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
            if approx > MAX_PAYLOAD_BYTES {
                return Err(EnvelopeError::TooLarge(approx));
            }
            parse_envelope_value(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_payload() -> &'static [u8] {
        br#"{"v":1,"id":"01J1","title":{"nl":"Deur","en":"Door"}}"#
    }

    #[test]
    fn a_minimal_valid_envelope_parses() {
        let env = parse_envelope(ok_payload()).unwrap();
        assert_eq!(env.id, "01J1");
        assert_eq!(env.title.unwrap().nl.as_deref(), Some("Deur"));
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compat() {
        let env = parse_envelope(
            br#"{"v":1,"id":"x","message":{"en":"hi"},"brand_new_field":{"deep":true}}"#,
        );
        assert!(env.is_ok());
    }

    #[test]
    fn non_json_is_malformed() {
        assert!(matches!(
            parse_envelope(b"not json"),
            Err(EnvelopeError::Malformed(_))
        ));
    }

    #[test]
    fn a_missing_v_is_its_own_error_for_the_dead_letter_log() {
        assert_eq!(
            parse_envelope(br#"{"id":"x","title":{"nl":"a"}}"#),
            Err(EnvelopeError::MissingVersion)
        );
    }

    #[test]
    fn ar4_an_oversized_payload_is_poisoned_without_parsing() {
        let huge = format!(
            r#"{{"v":1,"id":"x","title":{{"nl":"a"}},"data":"{}"}}"#,
            "z".repeat(MAX_PAYLOAD_BYTES)
        );
        assert!(matches!(
            parse_envelope(huge.as_bytes()),
            Err(EnvelopeError::TooLarge(_))
        ));
    }

    #[test]
    fn ar5_hub_payload_variants_funnel_correctly() {
        use crate::hub::HubPayload;
        assert_eq!(
            parse_from_hub(&HubPayload::Binary),
            Err(EnvelopeError::BinaryPayload)
        );
        let ok = parse_from_hub(&HubPayload::Text(
            r#"{"v":1,"id":"x","title":{"nl":"a"}}"#.into(),
        ));
        assert!(ok.is_ok());
        let v: serde_json::Value =
            serde_json::from_str(r#"{"v":1,"id":"y","message":{"en":"m"}}"#).unwrap();
        assert!(parse_from_hub(&HubPayload::Json(v)).is_ok());
    }

    #[test]
    fn a_future_version_is_refused() {
        assert_eq!(
            parse_envelope(br#"{"v":2,"id":"x","title":{"nl":"a"}}"#),
            Err(EnvelopeError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn an_empty_or_missing_id_is_refused() {
        assert_eq!(
            parse_envelope(br#"{"v":1,"id":"  ","title":{"nl":"a"}}"#),
            Err(EnvelopeError::MissingId)
        );
        assert_eq!(
            parse_envelope(br#"{"v":1,"title":{"nl":"a"}}"#),
            Err(EnvelopeError::MissingId)
        );
    }

    #[test]
    fn no_text_in_any_language_is_nothing_to_render() {
        assert_eq!(
            parse_envelope(br#"{"v":1,"id":"x","title":{"nl":"  "},"message":{}}"#),
            Err(EnvelopeError::NothingToRender)
        );
        assert_eq!(
            parse_envelope(br#"{"v":1,"id":"x"}"#),
            Err(EnvelopeError::NothingToRender)
        );
    }

    #[test]
    fn every_error_carries_a_remedy() {
        for err in [
            EnvelopeError::Malformed("x".into()),
            EnvelopeError::MissingVersion,
            EnvelopeError::UnsupportedVersion(9),
            EnvelopeError::MissingId,
            EnvelopeError::NothingToRender,
            EnvelopeError::TooLarge(1),
            EnvelopeError::BinaryPayload,
        ] {
            assert!(!err.remedy().is_empty());
        }
    }
}
