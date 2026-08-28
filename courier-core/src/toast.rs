//! Envelope → toast mapping (AR11, M1, M3): language pick with
//! cross-language fallback, priority → urgency/expire table.

use crate::envelope::{Envelope, LocalizedText};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Nl,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Normal,
    Critical,
}

impl Urgency {
    pub fn as_notify_send_arg(self) -> &'static str {
        match self {
            Urgency::Normal => "normal",
            Urgency::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastSpec {
    pub summary: String,
    /// Empty string = no body line (notify-send renders summary only).
    pub body: String,
    pub urgency: Urgency,
    /// 0 = persistent until dismissed (the critical case).
    pub expire_ms: u32,
}

fn pick(text: Option<&LocalizedText>, lang: Language) -> Option<String> {
    let t = text?;
    let (first, second) = match lang {
        Language::Nl => (&t.nl, &t.en),
        Language::En => (&t.en, &t.nl),
    };
    [first, second]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Priority → (urgency, expire) per the AR11 table. Unknown or absent
/// priorities read as `info` — tolerant in the same direction as AR4.
fn urgency_expire(priority: Option<&str>) -> (Urgency, u32) {
    match priority {
        Some("critical") => (Urgency::Critical, 0),
        Some("warning") => (Urgency::Normal, 30_000),
        _ => (Urgency::Normal, 10_000),
    }
}

/// AR4 truncation budget: a toast never needs more.
pub const SUMMARY_MAX_CHARS: usize = 200;
pub const BODY_MAX_CHARS: usize = 1000;

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Plasma parses a markup subset in the body (AR12): a payload's bare
/// `<` or `&` would render wrong or truncated, so the body ships
/// escaped. The summary is plain text everywhere and stays literal.
fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Only call with a parse-validated envelope: `parse_envelope`
/// guarantees at least one renderable text, so this cannot come up
/// empty. Title missing → the message text becomes the summary.
pub fn toast_spec(env: &Envelope, lang: Language) -> ToastSpec {
    let title = pick(env.title.as_ref(), lang);
    let message = pick(env.message.as_ref(), lang);
    let (summary, body) = match (title, message) {
        (Some(t), Some(m)) => (t, m),
        (Some(t), None) => (t, String::new()),
        (None, Some(m)) => (m, String::new()),
        (None, None) => unreachable!("parse_envelope guarantees renderable text"),
    };
    let (urgency, expire_ms) = urgency_expire(env.priority.as_deref());
    ToastSpec {
        summary: truncate(&summary, SUMMARY_MAX_CHARS),
        body: escape_markup(&truncate(&body, BODY_MAX_CHARS)),
        urgency,
        expire_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::parse_envelope;

    fn env(json: &str) -> Envelope {
        parse_envelope(json.as_bytes()).unwrap()
    }

    #[test]
    fn m3_nl_config_picks_nl_text() {
        let e = env(
            r#"{"v":1,"id":"x","title":{"nl":"Deur","en":"Door"},"message":{"nl":"Voordeur open","en":"Front door open"}}"#,
        );
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(t.summary, "Deur");
        assert_eq!(t.body, "Voordeur open");
    }

    #[test]
    fn m3_missing_nl_falls_back_to_en_rather_than_dropping() {
        let e = env(r#"{"v":1,"id":"x","title":{"en":"Door"}}"#);
        assert_eq!(toast_spec(&e, Language::Nl).summary, "Door");
    }

    #[test]
    fn m3_en_config_prefers_en() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"Deur","en":"Door"}}"#);
        assert_eq!(toast_spec(&e, Language::En).summary, "Door");
    }

    #[test]
    fn a_title_only_envelope_has_no_body() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"Deur"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.summary.as_str(), t.body.as_str()), ("Deur", ""));
    }

    #[test]
    fn a_message_only_envelope_promotes_message_to_summary() {
        let e = env(r#"{"v":1,"id":"x","message":{"nl":"Wasmachine klaar"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(
            (t.summary.as_str(), t.body.as_str()),
            ("Wasmachine klaar", "")
        );
    }

    #[test]
    fn m1_info_maps_to_normal_10s() {
        let e = env(r#"{"v":1,"id":"x","priority":"info","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.urgency, t.expire_ms), (Urgency::Normal, 10_000));
    }

    #[test]
    fn m1_warning_maps_to_normal_30s() {
        let e = env(r#"{"v":1,"id":"x","priority":"warning","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.urgency, t.expire_ms), (Urgency::Normal, 30_000));
    }

    #[test]
    fn m1_critical_maps_to_critical_persistent() {
        let e = env(r#"{"v":1,"id":"x","priority":"critical","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.urgency, t.expire_ms), (Urgency::Critical, 0));
    }

    #[test]
    fn ar12_the_body_is_markup_escaped_the_summary_is_not() {
        let e = env(
            r#"{"v":1,"id":"x","title":{"nl":"5 < 7 & zo"},"message":{"nl":"<b>dik</b> & meer"}}"#,
        );
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(t.summary, "5 < 7 & zo");
        assert_eq!(t.body, "&lt;b&gt;dik&lt;/b&gt; &amp; meer");
    }

    #[test]
    fn ar4_summary_and_body_are_truncated_with_an_ellipsis() {
        let long_title = "t".repeat(300);
        let long_msg = "m".repeat(1500);
        let e = env(&format!(
            r#"{{"v":1,"id":"x","title":{{"nl":"{long_title}"}},"message":{{"nl":"{long_msg}"}}}}"#
        ));
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(t.summary.chars().count(), SUMMARY_MAX_CHARS);
        assert!(t.summary.ends_with('…'));
        assert_eq!(t.body.chars().count(), BODY_MAX_CHARS);
        assert!(t.body.ends_with('…'));
    }

    #[test]
    fn m1_unknown_priority_reads_as_info() {
        let e = env(r#"{"v":1,"id":"x","priority":"shouting","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.urgency, t.expire_ms), (Urgency::Normal, 10_000));
    }
}
