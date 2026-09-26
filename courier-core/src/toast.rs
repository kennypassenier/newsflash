//! Envelope → toast mapping (AR11, M1, M3): language pick with
//! cross-language fallback, priority → urgency/expire table. M10
//! (2026-08-30, pipeline-v2 K12): every toast carries action buttons —
//! the envelope's own `actions` if present (max 2), else the default
//! "gelezen"/"snooze" pair.
//!
//! AR11 amendment (2026-08-30, mini-round): each priority now also
//! carries a distinct icon. First attempt was a `low`/`normal` urgency
//! split for `info`/`warning` — reverted the same day, live-confirmed
//! against Plasma's own QML source that urgency carries no visual
//! styling at all (only behavioural differences: persistence, DND
//! bypass, sort order). The icon is the part that actually renders
//! differently (see `docs/DRILL_LOG.md`).

use crate::envelope::{ActionDef, Envelope, LocalizedText};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Nl,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// feat-8 quiet delivery: Plasma's default is no popup for low
    /// urgency, straight into the history.
    Low,
    Normal,
    Critical,
}

impl Urgency {
    pub fn as_notify_send_arg(self) -> &'static str {
        match self {
            Urgency::Low => "low",
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
    /// AR11 amendment (2026-08-30): a freedesktop icon name, one per
    /// priority — the actual visual differentiator (urgency itself
    /// carries no Plasma styling, live-confirmed against Plasma's own
    /// QML source).
    pub icon: &'static str,
    /// M10: (action id, resolved label). Never empty — always the
    /// custom pair or the default one.
    pub actions: Vec<(String, String)>,
}

/// The reserved default pair (M10/K12): shown when the envelope carries
/// no `actions`. Fixed labels, not run through the nl/en pick logic —
/// pipeline-v2's contract spells these two words exactly.
pub const DEFAULT_ACTIONS: [(&str, &str); 2] = [("gelezen", "Gelezen"), ("snooze", "Snooze")];

/// Buttons shown at most, on both desktops. Was 2 (K12's "max 2");
/// raised to 5 on 2026-09-24 because pipeline-v2's live messages carry 3
/// (plant care: Water gegeven / Snooze 3 dagen / Nog te nat), Windows
/// allows 5, and Plasma was measured rendering 20 (D3, labels blur past
/// ~6-8). One number for both OSes.
pub const MAX_ACTIONS: usize = 5;

/// M10: producer-supplied actions beyond `MAX_ACTIONS` are dropped — the
/// caller (which has logging) should warn when this returns true.
pub fn actions_are_truncated(env: &Envelope) -> bool {
    env.effective_actions()
        .is_some_and(|a| a.len() > MAX_ACTIONS)
}

/// How long a notification may exist (config, shared by both desktops).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetimes {
    /// For `ephemeral: true` messages without their own lifetime.
    pub ephemeral: u32,
    /// Per priority; `None` = the desktop's own default (Windows keeps a
    /// toast 3 days, Plasma keeps history until cleared).
    pub info: Option<u32>,
    pub warning: Option<u32>,
    pub critical: Option<u32>,
}

/// How a message reaches the desktop (feat-8, Kenny 2026-09-26: the
/// desktop follows Home Assistant's own gate). `critical` always pops
/// up, whatever the gate said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Popup,
    /// No popup and no sound: straight into the notification history.
    Quiet,
    /// Home Assistant discarded it; the desktop acks and shows nothing.
    Skip,
}

pub fn presentation(env: &Envelope) -> Presentation {
    if env.priority.as_deref() == Some("critical") {
        return Presentation::Popup;
    }
    match env.gate_outcome.as_deref() {
        Some("deferred") => Presentation::Quiet,
        Some("dropped") => Presentation::Skip,
        _ => Presentation::Popup,
    }
}

/// How long a popup stays on screen before it moves to the history
/// (feat-7). `critical` has no entry on purpose: it stays until answered,
/// the promise `critical` exists for (Kenny, 2026-09-26: only info and
/// warning are tunable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupDurations {
    pub info_ms: u32,
    pub warning_ms: u32,
}

pub const DEFAULT_POPUP_INFO_SECONDS: u32 = 10;
pub const DEFAULT_POPUP_WARNING_SECONDS: u32 = 30;
/// An hour on screen is already a sticky note; more is a config typo.
pub const MAX_POPUP_SECONDS: u32 = 3600;

impl Default for PopupDurations {
    fn default() -> Self {
        PopupDurations {
            info_ms: DEFAULT_POPUP_INFO_SECONDS * 1000,
            warning_ms: DEFAULT_POPUP_WARNING_SECONDS * 1000,
        }
    }
}

pub const DEFAULT_EPHEMERAL_MINUTES: u32 = 10;
/// Windows keeps a toast at most 3 days; a longer lifetime means nothing.
pub const MAX_LIFETIME_MINUTES: u32 = 3 * 24 * 60;

impl Default for Lifetimes {
    fn default() -> Self {
        Lifetimes {
            ephemeral: DEFAULT_EPHEMERAL_MINUTES,
            info: None,
            warning: None,
            critical: None,
        }
    }
}

/// Minutes this message may exist after publishing, most specific
/// first: the message's own `expires_in_minutes`, then `ephemeral`,
/// then the config for its priority. Clamped to 1 minute … 3 days.
pub fn lifetime_minutes(env: &Envelope, lifetimes: &Lifetimes) -> Option<u32> {
    let per_priority = match env.priority.as_deref() {
        Some("critical") => lifetimes.critical,
        Some("warning") => lifetimes.warning,
        _ => lifetimes.info,
    };
    let minutes = env
        .expires_in_minutes
        .or((env.ephemeral == Some(true)).then_some(lifetimes.ephemeral))
        .or(per_priority)?;
    Some(minutes.clamp(1, MAX_LIFETIME_MINUTES))
}

/// A link a notification may open. Absolute http(s) URLs as they are;
/// a path (`/control-panel/homelab`, as pipeline-v2 sends today) only
/// when a `link_base_url` is configured to resolve it against.
pub fn resolve_link(url: &str, base: Option<&str>) -> Option<String> {
    let url = url.trim();
    let full = if url.starts_with('/') {
        format!("{}{url}", base?.trim_end_matches('/'))
    } else {
        url.to_string()
    };
    let lower = full.to_ascii_lowercase();
    ((lower.starts_with("http://") || lower.starts_with("https://"))
        && full.len() > "http://".len()
        && full.len() <= 2048
        && !full.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then_some(full)
}

/// M10 safety-cap policy (standing rule 27 — the margin is the
/// operational knob, the "no cap for persistent toasts" branch is a
/// pinned decision, not configurable): a bounded toast's interactive
/// watcher waits `expire_ms` plus a margin above what the toast already
/// promises to do on its own. A persistent (critical, `expire_ms == 0`)
/// toast gets **no cap** — measured live 2026-08-30: once the watcher
/// kills the process, Plasma drops the action buttons even though the
/// notification body stays visible, silently breaking "critical lasts
/// until dismissed" long before anyone answers.
pub fn interactive_wait_cap_ms(expire_ms: u32, margin_ms: u32) -> Option<u32> {
    if expire_ms == 0 {
        None
    } else {
        Some(expire_ms.saturating_add(margin_ms))
    }
}

fn resolve_actions(env: &Envelope, lang: Language) -> Vec<(String, String)> {
    match env.effective_actions() {
        Some(defs) => defs
            .iter()
            .take(MAX_ACTIONS)
            .map(|d: &ActionDef| {
                let label = pick(Some(&d.label), lang).unwrap_or_else(|| d.id.clone());
                (d.id.clone(), label)
            })
            .collect(),
        None => DEFAULT_ACTIONS
            .iter()
            .map(|(id, label)| (id.to_string(), label.to_string()))
            .collect(),
    }
}

/// Language pick with cross-language fallback (M3). Public so the
/// Windows toast builder resolves text exactly the same way.
pub fn pick(text: Option<&LocalizedText>, lang: Language) -> Option<String> {
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

/// Priority → (urgency, expire) per the AR11 table, with the popup
/// durations from the config (feat-7). Unknown or absent priorities
/// read as `info` — tolerant in the same direction as AR4. `critical`
/// stays pinned at 0 (persistent), whatever the config says.
fn urgency_expire(priority: Option<&str>, popup: &PopupDurations) -> (Urgency, u32) {
    match priority {
        Some("critical") => (Urgency::Critical, 0),
        Some("warning") => (Urgency::Normal, popup.warning_ms),
        _ => (Urgency::Normal, popup.info_ms),
    }
}

/// Priority → freedesktop icon name (AR11 amendment, 2026-08-30): the
/// visual differentiator urgency itself cannot provide. Standard
/// `dialog-*` names ship with every icon theme.
fn icon_for(priority: Option<&str>) -> &'static str {
    match priority {
        Some("critical") => "dialog-error",
        Some("warning") => "dialog-warning",
        _ => "dialog-information",
    }
}

/// AR4 truncation budget: a toast never needs more.
pub const SUMMARY_MAX_CHARS: usize = 200;
pub const BODY_MAX_CHARS: usize = 1000;

pub fn truncate(s: &str, max_chars: usize) -> String {
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

/// (summary, body), truncated but NOT escaped — each renderer applies
/// its own escaping. Only call with a parse-validated envelope:
/// `parse_envelope` guarantees at least one renderable text, so this
/// cannot come up empty. Title missing → the message text becomes the
/// summary.
pub fn resolve_texts(env: &Envelope, lang: Language) -> (String, String) {
    let title = pick(env.title.as_ref(), lang);
    let message = pick(env.message.as_ref(), lang);
    let (summary, body) = match (title, message) {
        (Some(t), Some(m)) => (t, m),
        (Some(t), None) => (t, String::new()),
        (None, Some(m)) => (m, String::new()),
        (None, None) => unreachable!("parse_envelope guarantees renderable text"),
    };
    (
        truncate(&summary, SUMMARY_MAX_CHARS),
        truncate(&body, BODY_MAX_CHARS),
    )
}

pub fn toast_spec(env: &Envelope, lang: Language) -> ToastSpec {
    toast_spec_with(env, lang, &PopupDurations::default())
}

pub fn toast_spec_with(env: &Envelope, lang: Language, popup: &PopupDurations) -> ToastSpec {
    let (summary, body) = resolve_texts(env, lang);
    let (urgency, expire_ms) = urgency_expire(env.priority.as_deref(), popup);
    let urgency = if presentation(env) == Presentation::Quiet {
        Urgency::Low
    } else {
        urgency
    };
    ToastSpec {
        summary,
        body: escape_markup(&body),
        urgency,
        expire_ms,
        icon: icon_for(env.priority.as_deref()),
        actions: resolve_actions(env, lang),
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
    fn feat_8_the_gate_decides_quiet_skip_or_popup_and_critical_always_pops() {
        let p = |json: &str| presentation(&env(json));
        assert_eq!(
            p(r#"{"v":1,"id":"x","gate_outcome":"live","title":{"nl":"a"}}"#),
            Presentation::Popup
        );
        assert_eq!(
            p(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#),
            Presentation::Popup
        );
        assert_eq!(
            p(r#"{"v":1,"id":"x","gate_outcome":"deferred","title":{"nl":"a"}}"#),
            Presentation::Quiet
        );
        assert_eq!(
            p(r#"{"v":1,"id":"x","gate_outcome":"dropped","title":{"nl":"a"}}"#),
            Presentation::Skip
        );
        assert_eq!(
            p(
                r#"{"v":1,"id":"x","priority":"critical","gate_outcome":"deferred","title":{"nl":"a"}}"#
            ),
            Presentation::Popup
        );
        let quiet = env(
            r#"{"v":1,"id":"x","priority":"warning","gate_outcome":"deferred","title":{"nl":"a"}}"#,
        );
        assert_eq!(toast_spec(&quiet, Language::Nl).urgency, Urgency::Low);
    }

    #[test]
    fn feat_7_configured_popup_durations_apply_to_info_and_warning_only() {
        let popup = PopupDurations {
            info_ms: 4_000,
            warning_ms: 60_000,
        };
        let spec = |p: &str| {
            let e = env(&format!(
                r#"{{"v":1,"id":"x","priority":"{p}","title":{{"nl":"a"}}}}"#
            ));
            let t = toast_spec_with(&e, Language::Nl, &popup);
            (t.urgency, t.expire_ms)
        };
        assert_eq!(spec("info"), (Urgency::Normal, 4_000));
        assert_eq!(spec("warning"), (Urgency::Normal, 60_000));
        assert_eq!(spec("shouting"), (Urgency::Normal, 4_000));
        assert_eq!(spec("critical"), (Urgency::Critical, 0));
    }

    #[test]
    fn m1_critical_maps_to_critical_persistent() {
        let e = env(r#"{"v":1,"id":"x","priority":"critical","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!((t.urgency, t.expire_ms), (Urgency::Critical, 0));
    }

    #[test]
    fn ar11_the_three_priorities_get_three_distinct_icons() {
        assert_eq!(icon_for(Some("info")), "dialog-information");
        assert_eq!(icon_for(Some("warning")), "dialog-warning");
        assert_eq!(icon_for(Some("critical")), "dialog-error");
    }

    #[test]
    fn ar11_an_unknown_priority_gets_the_info_icon() {
        assert_eq!(icon_for(Some("shouting")), "dialog-information");
        assert_eq!(icon_for(None), "dialog-information");
    }

    #[test]
    fn ar11_toast_spec_carries_the_icon_for_its_priority() {
        let e = env(r#"{"v":1,"id":"x","priority":"warning","title":{"nl":"a"}}"#);
        assert_eq!(toast_spec(&e, Language::Nl).icon, "dialog-warning");
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

    #[test]
    fn k12_no_actions_field_gets_the_default_pair() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(
            t.actions,
            vec![
                ("gelezen".to_string(), "Gelezen".to_string()),
                ("snooze".to_string(), "Snooze".to_string())
            ]
        );
    }

    #[test]
    fn k12_an_empty_actions_array_also_gets_the_default_pair() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[]}"#);
        assert_eq!(toast_spec(&e, Language::Nl).actions.len(), 2);
    }

    #[test]
    fn k12_custom_actions_replace_the_default_pair_with_localized_labels() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[
                {"id":"ik_pak_het_op","label":{"nl":"Ik pak het op","en":"I'm on it"}}
            ]}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(
            t.actions,
            vec![("ik_pak_het_op".to_string(), "Ik pak het op".to_string())]
        );
        let t_en = toast_spec(&e, Language::En);
        assert_eq!(t_en.actions[0].1, "I'm on it");
    }

    #[test]
    fn more_than_max_actions_are_truncated_and_flagged() {
        let buttons: Vec<String> = (0..7)
            .map(|i| format!(r#"{{"id":"b{i}","label":{{"nl":"B{i}"}}}}"#))
            .collect();
        let e = env(&format!(
            r#"{{"v":1,"id":"x","title":{{"nl":"a"}},"actions":[{}]}}"#,
            buttons.join(",")
        ));
        assert!(actions_are_truncated(&e));
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(t.actions.len(), MAX_ACTIONS);
        assert_eq!(t.actions[0].0, "b0");
        assert_eq!(t.actions[4].0, "b4");
    }

    #[test]
    fn the_live_plant_care_message_gets_its_three_buttons() {
        let e = env(
            r#"{"v":1,"id":"x","title":{"nl":"2 planten"},"data":{"action_buttons":[
                {"action":"PLANTCARE_WATER_z","title":"Water gegeven"},
                {"action":"PLANTCARE_SNOOZE_z","title":"Snooze 3 dagen"},
                {"action":"PLANTCARE_SKIP_z","title":"Nog te nat"}]}}"#,
        );
        assert!(!actions_are_truncated(&e));
        let t = toast_spec(&e, Language::En);
        assert_eq!(
            t.actions,
            vec![
                ("PLANTCARE_WATER_z".to_string(), "Water gegeven".to_string()),
                (
                    "PLANTCARE_SNOOZE_z".to_string(),
                    "Snooze 3 dagen".to_string()
                ),
                ("PLANTCARE_SKIP_z".to_string(), "Nog te nat".to_string()),
            ],
            "single-language titles show whatever the language setting"
        );
    }

    #[test]
    fn lifetime_prefers_the_message_then_ephemeral_then_the_priority() {
        let l = Lifetimes {
            ephemeral: 10,
            info: Some(60),
            warning: None,
            critical: None,
        };
        let plain = env(r#"{"v":1,"id":"x","priority":"info","title":{"nl":"a"}}"#);
        assert_eq!(lifetime_minutes(&plain, &l), Some(60));
        let warn = env(r#"{"v":1,"id":"x","priority":"warning","title":{"nl":"a"}}"#);
        assert_eq!(
            lifetime_minutes(&warn, &l),
            None,
            "no setting: desktop default"
        );
        let eph =
            env(r#"{"v":1,"id":"x","priority":"critical","ephemeral":true,"title":{"nl":"a"}}"#);
        assert_eq!(lifetime_minutes(&eph, &l), Some(10));
        let own =
            env(r#"{"v":1,"id":"x","ephemeral":true,"expires_in_minutes":3,"title":{"nl":"a"}}"#);
        assert_eq!(lifetime_minutes(&own, &l), Some(3));
        let not_eph = env(r#"{"v":1,"id":"x","ephemeral":false,"title":{"nl":"a"}}"#);
        assert_eq!(lifetime_minutes(&not_eph, &Lifetimes::default()), None);
        let huge = env(r#"{"v":1,"id":"x","expires_in_minutes":0,"title":{"nl":"a"}}"#);
        assert_eq!(lifetime_minutes(&huge, &l), Some(1));
    }

    #[test]
    fn links_resolve_paths_only_against_a_configured_base() {
        let base = Some("http://10.10.10.2:8123/");
        assert_eq!(
            resolve_link("/control-panel/homelab", base).as_deref(),
            Some("http://10.10.10.2:8123/control-panel/homelab")
        );
        assert_eq!(resolve_link("/control-panel/homelab", None), None);
        assert_eq!(
            resolve_link("http://10.10.10.6:8989/activity/queue", None).as_deref(),
            Some("http://10.10.10.6:8989/activity/queue")
        );
        assert_eq!(resolve_link("", base), None);
        assert_eq!(resolve_link("file:///etc/passwd", base), None);
        assert_eq!(resolve_link("ms-settings:privacy", base), None);
        assert_eq!(resolve_link("/x y", base), None);
    }

    #[test]
    fn k12_two_actions_is_not_flagged_as_truncated() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[
                {"id":"a","label":{"nl":"A"}},
                {"id":"b","label":{"nl":"B"}}
            ]}"#);
        assert!(!actions_are_truncated(&e));
    }

    #[test]
    fn k12_a_custom_action_missing_its_label_falls_back_to_the_id() {
        let e = env(r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[{"id":"raw_id"}]}"#);
        let t = toast_spec(&e, Language::Nl);
        assert_eq!(
            t.actions,
            vec![("raw_id".to_string(), "raw_id".to_string())]
        );
    }

    #[test]
    fn m10_a_bounded_toast_gets_expire_plus_margin() {
        assert_eq!(interactive_wait_cap_ms(10_000, 30_000), Some(40_000));
        assert_eq!(interactive_wait_cap_ms(30_000, 30_000), Some(60_000));
    }

    #[test]
    fn m10_a_persistent_critical_toast_gets_no_cap() {
        assert_eq!(interactive_wait_cap_ms(0, 30_000), None);
    }
}
