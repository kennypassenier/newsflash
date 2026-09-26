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
    #[serde(default)]
    pub ack_id: Option<String>,
    /// M10 (pipeline-v2 K12, 2026-08-30): optional custom action buttons,
    /// max 2. Absent/empty → the toast layer fills in the default
    /// "gelezen"/"snooze" pair (AR11 amendment) — this field only carries
    /// an override.
    #[serde(default)]
    pub actions: Option<Vec<ActionDef>>,
    // Windows-port extensions (W-series, docs/WINDOWS.md): optional,
    // PROPOSED to pipeline-v2, not part of the ratified v1 contract.
    // The Linux renderer ignores them all. Each one is read leniently —
    // a wrong-typed extension is dropped, never poison — so no producer
    // experiment can dead-letter a message that renders fine without it.
    /// feat-win-5: body click opens this http(s) URL (already in the v1 draft).
    #[serde(default, deserialize_with = "lenient")]
    pub click_url: Option<String>,
    /// feat-win-6: hero image, fetched by the courier (plain http, LAN only).
    #[serde(default, deserialize_with = "lenient")]
    pub image: Option<String>,
    /// feat-win-7: a later toast with the same tag replaces this one in place.
    #[serde(default, deserialize_with = "lenient")]
    pub tag: Option<String>,
    /// feat-win-8: progress bar.
    #[serde(default, deserialize_with = "lenient")]
    pub progress: Option<Progress>,
    /// feat-win-9: text boxes / dropdowns; values ride back on the click.
    #[serde(default, deserialize_with = "lenient")]
    pub inputs: Option<Vec<InputDef>>,
    /// Live pipeline-v2 field (seen on the hub since 2026-08-29): the
    /// message is short-lived and must not linger in notification
    /// history. How long it may exist is the config's `ephemeral_minutes`
    /// unless the message says so itself (`expires_in_minutes`).
    #[serde(default, deserialize_with = "lenient")]
    pub ephemeral: Option<bool>,
    /// feat-win-12 (PROPOSED to pipeline-v2): an explicit lifetime in minutes —
    /// the desktop drops the notification this long after publishing.
    #[serde(default, deserialize_with = "lenient")]
    pub expires_in_minutes: Option<u32>,
    /// pipeline-v2's gate verdict at publish time (feat-8): `live`,
    /// `deferred` (Do Not Disturb, outside the active hours, or a media
    /// session: parked for the hourly bulletin) or `dropped` (an
    /// ephemeral message under those conditions, discarded by Home
    /// Assistant). Measured in `script.notification_dispatch`, 2026-09-26.
    #[serde(default, deserialize_with = "lenient")]
    pub gate_outcome: Option<String>,
    /// pipeline-v2's channel data. The courier reads exactly one thing
    /// from it: the action buttons (`data.action_buttons`). Everything
    /// else in it (lights, speakers, push targets) belongs to other
    /// channels — no routing here (S9). `tts` is not modeled at all:
    /// speech is the DLNA channel's job.
    #[serde(default, deserialize_with = "lenient")]
    pub data: Option<EnvelopeData>,
}

/// The part of `data` the courier uses.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EnvelopeData {
    #[serde(default, deserialize_with = "lenient")]
    pub action_buttons: Option<Vec<DataButton>>,
}

/// pipeline-v2's live button format (the HA companion-app shape):
/// `{"action": "PLANTCARE_WATER_…", "title": "Water gegeven"}`, plus
/// the companion app's `uri` for link buttons (`"action": "URI"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataButton {
    pub action: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub uri: Option<String>,
}

impl Envelope {
    /// The buttons this message asks for: the K12 `actions` field when
    /// present, else pipeline-v2's live `data.action_buttons`, else
    /// `None` (the renderer falls back to the default Gelezen/Snooze).
    pub fn effective_actions(&self) -> Option<Vec<ActionDef>> {
        if let Some(actions) = self.actions.as_ref().filter(|a| !a.is_empty()) {
            return Some(actions.clone());
        }
        let buttons = self
            .data
            .as_ref()?
            .action_buttons
            .as_ref()
            .filter(|b| !b.is_empty())?;
        Some(
            buttons
                .iter()
                .filter(|b| !b.action.trim().is_empty())
                .map(|b| ActionDef {
                    id: b.action.clone(),
                    label: LocalizedText {
                        nl: Some(b.title.clone()).filter(|t| !t.trim().is_empty()),
                        en: None,
                    },
                    style: None,
                    url: b
                        .uri
                        .clone()
                        .filter(|_| b.action.eq_ignore_ascii_case("URI")),
                })
                .collect(),
        )
        .filter(|v: &Vec<ActionDef>| !v.is_empty())
    }
}

/// Wrong type → `None` instead of a deserialize error (see the
/// extension note on `Envelope`).
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// feat-win-8. `value` is a fraction 0.0–1.0 or the string "indeterminate";
/// kept as raw JSON so a bad value degrades to indeterminate, not poison.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Progress {
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub status: LocalizedText,
    #[serde(default)]
    pub title: Option<LocalizedText>,
    /// Replaces the default "60%" text, e.g. "3/5 files".
    #[serde(default)]
    pub label: Option<String>,
}

/// feat-win-9. `kind` is "text" (default) or "selection".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InputDef {
    pub id: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<LocalizedText>,
    #[serde(default)]
    pub placeholder: Option<LocalizedText>,
    #[serde(default)]
    pub choices: Vec<ChoiceDef>,
    /// Preselected choice id (selection) or prefilled text (text).
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChoiceDef {
    pub id: String,
    #[serde(default)]
    pub label: LocalizedText,
}

/// One custom action button (M10). `id` rides back on the click as-is;
/// "gelezen" and "snooze" are reserved by pipeline-v2's contract and
/// keep their dedicated HA-side handling — any other id is producer-
/// defined and replays as a `mobile_app_notification_action` event.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ActionDef {
    pub id: String,
    #[serde(default)]
    pub label: LocalizedText,
    /// feat-win-3 (Windows only): "success" (green) or "critical" (red).
    #[serde(default, deserialize_with = "lenient")]
    pub style: Option<String>,
    /// feat-win-4 (Windows only): the button opens this http(s) URL instead of
    /// replying to `notify.actions`.
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
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
    fn w_windows_extensions_parse_when_well_formed() {
        let env = parse_envelope(
            br#"{"v":1,"id":"x","title":{"nl":"a"},"click_url":"http://ha.local/",
                "image":"http://ha.local/cam.jpg","tag":"wasmachine",
                "progress":{"value":0.4,"status":{"nl":"Wassen"}},
                "inputs":[{"id":"reply","type":"text"}],
                "actions":[{"id":"ok","style":"success"}],"expires_in_minutes":15}"#,
        )
        .unwrap();
        assert_eq!(env.tag.as_deref(), Some("wasmachine"));
        assert_eq!(env.expires_in_minutes, Some(15));
        assert_eq!(env.progress.unwrap().status.nl.as_deref(), Some("Wassen"));
        assert_eq!(env.inputs.unwrap()[0].id, "reply");
        assert_eq!(env.actions.unwrap()[0].style.as_deref(), Some("success"));
    }

    #[test]
    fn w_a_wrong_typed_extension_is_dropped_never_poison() {
        let env = parse_envelope(
            br#"{"v":1,"id":"x","title":{"nl":"a"},"click_url":42,"progress":"half",
                "inputs":{"not":"a list"},"tag":["x"],"actions":[{"id":"ok","style":7}],
                "expires_in_minutes":-5}"#,
        )
        .unwrap();
        assert_eq!(env.click_url, None);
        assert_eq!(env.progress, None);
        assert_eq!(env.inputs, None);
        assert_eq!(env.tag, None);
        assert_eq!(env.expires_in_minutes, None);
        assert_eq!(env.actions.unwrap()[0].style, None);
    }

    #[test]
    fn live_data_action_buttons_become_the_buttons_unless_actions_is_set() {
        let env = parse_envelope(
            br#"{"v":1,"id":"x","title":{"nl":"a"},"data":{"action_buttons":[
                {"action":"PLANTCARE_WATER_x","title":"Water gegeven"},
                {"action":"URI","title":"Open","uri":"http://ha/x"}],"color":"Blue"}}"#,
        )
        .unwrap();
        let actions = env.effective_actions().unwrap();
        assert_eq!(actions[0].id, "PLANTCARE_WATER_x");
        assert_eq!(actions[0].label.nl.as_deref(), Some("Water gegeven"));
        assert_eq!(actions[1].url.as_deref(), Some("http://ha/x"));

        let both = parse_envelope(
            br#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[{"id":"k12"}],
                "data":{"action_buttons":[{"action":"live","title":"L"}]}}"#,
        )
        .unwrap();
        assert_eq!(both.effective_actions().unwrap()[0].id, "k12");

        let empty =
            parse_envelope(br#"{"v":1,"id":"x","title":{"nl":"a"},"data":{"action_buttons":[]}}"#)
                .unwrap();
        assert_eq!(empty.effective_actions(), None);
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
