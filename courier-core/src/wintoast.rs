//! Envelope → Windows toast XML, and the button-argument codec (the
//! Windows port, docs/WINDOWS.md). Pure like the rest of core: the
//! newsflash-win shell hands in what it already resolved (asset paths,
//! a downloaded hero image) and gets back a string for
//! `XmlDocument::LoadXml`. Everything a toast looks like is decided —
//! and tested — here, on any OS.
//!
//! Priority mapping (the Windows reading of AR11): `info` → short
//! popup (~7 s), `warning` → long popup (~25 s), `critical` → a
//! scenario that stays on screen until answered (`reminder` by
//! default; `urgent` breaks through Do Not Disturb instead; `alarm`
//! also loops a sound). Windows has no free-form expire time, so these
//! are the closest honest equivalents of 10 s / 30 s / persistent.

use crate::envelope::{ActionDef, Envelope, InputDef, Progress};
use crate::toast::{
    DEFAULT_ACTIONS, Language, Lifetimes, MAX_ACTIONS, PopupDurations, Presentation,
    lifetime_minutes, pick, presentation, resolve_link, resolve_texts,
};

/// Platform limits (toast schema): buttons + context-menu items, inputs,
/// and items per selection input.
pub const MAX_BUTTONS: usize = MAX_ACTIONS;
pub const MAX_INPUTS: usize = 5;
pub const MAX_CHOICES: usize = 5;
/// `ToastNotification.Tag` is capped at 64 UTF-16 units.
pub const MAX_TAG_UNITS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriticalScenario {
    /// Stays on screen until dismissed or answered (the default — the
    /// closest match to Linux's persistent critical toast).
    Reminder,
    /// Windows 11 "important notification": breaks through Do Not
    /// Disturb (the user is asked once to allow it).
    Urgent,
    /// Like reminder, plus a looping alarm sound.
    Alarm,
}

impl CriticalScenario {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "reminder" => Some(CriticalScenario::Reminder),
            "urgent" => Some(CriticalScenario::Urgent),
            "alarm" => Some(CriticalScenario::Alarm),
            _ => None,
        }
    }

    fn attr(self) -> &'static str {
        match self {
            CriticalScenario::Reminder => "reminder",
            CriticalScenario::Urgent => "urgent",
            CriticalScenario::Alarm => "alarm",
        }
    }
}

/// What the shell resolved before building.
#[derive(Debug, Clone)]
pub struct BuildInput<'a> {
    pub hub_id: &'a str,
    pub published_at_ms: u64,
    pub language: Language,
    pub critical_scenario: CriticalScenario,
    /// Local path (or file URI) of the per-priority logo (`logo_asset`).
    pub logo_uri: Option<&'a str>,
    /// Local path (or file URI) of the downloaded hero image (feat-win-6).
    pub hero_uri: Option<&'a str>,
    /// newsflash plays its own chime (K6 `sound_file`) — keep the toast
    /// itself silent so there are never two sounds.
    pub silent: bool,
    /// Demo toasts: clicks are logged, never published to the hub.
    pub demo: bool,
    /// How long notifications may exist (config; see `lifetime_minutes`).
    pub lifetimes: Lifetimes,
    /// Popup durations from the config (feat-7); see `windows_duration`.
    pub popup: PopupDurations,
    /// Resolves path-only links (`/control-panel/homelab`); see
    /// `toast::resolve_link`.
    pub link_base: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinToast {
    pub xml: String,
    /// feat-win-7: `ToastNotification.Tag` — same tag replaces in place.
    pub tag: Option<String>,
    /// feat-win-8 live update: non-empty when the toast is data-bound (tag +
    /// progress). The XML then carries `{key}` placeholders and the
    /// shell first tries `ToastNotifier.Update` with these values — a
    /// silent in-place refresh of the toast already on screen — and
    /// only shows a new toast when there is none to update.
    pub data: Vec<(String, String)>,
    /// Producer actions / inputs beyond the platform limits (the shell
    /// logs these, AR27's "truncate and log, never poison").
    pub dropped_actions: usize,
    pub dropped_inputs: usize,
    /// feat-win-12: when Windows should drop the toast from Notification Center
    /// (Unix ms). `None` = Windows' own default (3 days).
    pub expires_at_ms: Option<u64>,
    /// feat-8: a deferred message goes straight to Notification Center
    /// (`ToastNotification.SuppressPopup`), silently.
    pub suppress_popup: bool,
}

/// Windows offers two popup durations only: `short` (about 7 s) and
/// `long` (about 25 s). A configured duration picks whichever it is
/// nearer to; the defaults (info 10 s, warning 30 s) keep the mapping
/// they had before feat-7 made them configurable.
pub const WINDOWS_LONG_FROM_MS: u32 = 16_000;

pub fn windows_duration(popup_ms: u32) -> &'static str {
    if popup_ms >= WINDOWS_LONG_FROM_MS {
        "long"
    } else {
        "short"
    }
}

/// When Windows should drop the toast (`ToastNotification.ExpirationTime`),
/// counted from the publish time — a message that waited at the hub does
/// not live longer than asked.
pub fn expires_at_ms(env: &Envelope, input: &BuildInput) -> Option<u64> {
    let minutes = lifetime_minutes(env, &input.lifetimes)?;
    Some(input.published_at_ms + u64::from(minutes) * 60_000)
}

/// Per-priority logo file name inside the assets directory (the AR11
/// icon amendment's Windows twin: the visual differentiator).
pub fn logo_asset(priority: Option<&str>) -> &'static str {
    match priority {
        Some("critical") => "critical.png",
        Some("warning") => "warning.png",
        _ => "info.png",
    }
}

/// feat-win-4/feat-win-5: only plain web links may be opened from a toast. A producer
/// on the hub must never be able to launch `file:`, `ms-settings:` or
/// any other protocol handler with one click.
pub fn is_openable_url(url: &str) -> bool {
    !url.trim().starts_with('/') && resolve_link(url, None).is_some()
}

pub fn build_toast(env: &Envelope, input: &BuildInput) -> WinToast {
    let (summary, body) = resolve_texts(env, input.language);
    let lang = input.language;
    let priority = env.priority.as_deref();
    let tag = env
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(clamp_tag);
    let live = tag.is_some() && env.progress.is_some();
    let mut data: Vec<(String, String)> = Vec::new();
    // Live toasts bind every changing text to a key; others inline it.
    let mut bind = |key: &str, value: String| -> String {
        if live {
            data.push((key.to_string(), value));
            format!("{{{key}}}")
        } else {
            value
        }
    };

    let mut toast_attrs: Vec<(&str, String)> = Vec::new();
    match env
        .click_url
        .as_deref()
        .and_then(|u| resolve_link(u, input.link_base))
    {
        Some(url) => {
            toast_attrs.push(("launch", url.to_string()));
            toast_attrs.push(("activationType", "protocol".into()));
        }
        None => toast_attrs.push((
            "launch",
            encode_activation(&Activation::Body {
                hub_id: input.hub_id.to_string(),
            }),
        )),
    }
    match priority {
        Some("critical") => toast_attrs.push(("scenario", input.critical_scenario.attr().into())),
        Some("warning") => {
            toast_attrs.push(("duration", windows_duration(input.popup.warning_ms).into()))
        }
        _ => toast_attrs.push(("duration", windows_duration(input.popup.info_ms).into())),
    }
    toast_attrs.push((
        "displayTimestamp",
        rfc3339_utc(input.published_at_ms / 1000),
    ));

    // Buttons first, so we know whether any carries a style.
    let (buttons, dropped_actions) = resolve_buttons(env, input);
    if buttons.iter().any(|b| b.contains("hint-buttonStyle")) {
        toast_attrs.push(("useButtonStyle", "true".into()));
    }

    let mut xml = String::from("<toast");
    for (k, v) in &toast_attrs {
        push_attr(&mut xml, k, v);
    }
    xml.push('>');

    // Header: groups toasts per producer in Notification Center.
    let source = env
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(src) = source {
        xml.push_str("<header");
        push_attr(&mut xml, "id", src);
        push_attr(&mut xml, "title", src);
        push_attr(
            &mut xml,
            "arguments",
            &encode_activation(&Activation::Header),
        );
        xml.push_str("/>");
    }

    xml.push_str(r#"<visual><binding template="ToastGeneric">"#);
    xml.push_str(r#"<text hint-maxLines="2">"#);
    xml.push_str(&escape(&bind("title", summary)));
    xml.push_str("</text>");
    if live || !body.is_empty() {
        xml.push_str("<text>");
        xml.push_str(&escape(&bind("body", body)));
        xml.push_str("</text>");
    }
    if let Some(src) = source {
        xml.push_str(r#"<text placement="attribution">"#);
        xml.push_str(&escape(&format!("via {src}")));
        xml.push_str("</text>");
    }
    if let Some(logo) = input.logo_uri {
        xml.push_str(r#"<image placement="appLogoOverride""#);
        push_attr(&mut xml, "src", logo);
        xml.push_str("/>");
    }
    if let Some(hero) = input.hero_uri {
        xml.push_str(r#"<image placement="hero""#);
        push_attr(&mut xml, "src", hero);
        xml.push_str("/>");
    }
    if let Some(progress) = &env.progress {
        xml.push_str(&progress_xml(progress, lang, &mut bind));
    }
    xml.push_str("</binding></visual>");

    let (inputs, dropped_inputs) = resolve_inputs(env.inputs.as_deref().unwrap_or(&[]), lang);
    xml.push_str("<actions>");
    for i in &inputs {
        xml.push_str(i);
    }
    for b in &buttons {
        xml.push_str(b);
    }
    xml.push_str("</actions>");

    if input.silent || presentation(env) == Presentation::Quiet {
        xml.push_str(r#"<audio silent="true"/>"#);
    }
    xml.push_str("</toast>");

    WinToast {
        xml,
        tag,
        data,
        dropped_actions,
        dropped_inputs,
        expires_at_ms: expires_at_ms(env, input),
        suppress_popup: presentation(env) == Presentation::Quiet,
    }
}

fn resolve_buttons(env: &Envelope, input: &BuildInput) -> (Vec<String>, usize) {
    let defs: Vec<ActionDef> = match env.effective_actions() {
        Some(defs) => defs,
        None => DEFAULT_ACTIONS
            .iter()
            .map(|(id, label)| ActionDef {
                id: id.to_string(),
                label: crate::envelope::LocalizedText {
                    nl: Some(label.to_string()),
                    en: Some(label.to_string()),
                },
                style: None,
                url: None,
            })
            .collect(),
    };
    let dropped = defs.len().saturating_sub(MAX_BUTTONS);
    let buttons = defs
        .iter()
        .take(MAX_BUTTONS)
        .map(|d| {
            let label = pick(Some(&d.label), input.language).unwrap_or_else(|| d.id.clone());
            let mut x = String::from("<action");
            push_attr(&mut x, "content", &label);
            match d
                .url
                .as_deref()
                .and_then(|u| resolve_link(u, input.link_base))
            {
                Some(url) => {
                    push_attr(&mut x, "activationType", "protocol");
                    push_attr(&mut x, "arguments", &url);
                }
                None => push_attr(
                    &mut x,
                    "arguments",
                    &encode_activation(&Activation::Button {
                        action_id: d.id.clone(),
                        envelope_id: env.id.clone(),
                        ack_id: env.ack_id.clone(),
                        hub_id: input.hub_id.to_string(),
                        demo: input.demo,
                    }),
                ),
            }
            match d.style.as_deref() {
                Some("success") => push_attr(&mut x, "hint-buttonStyle", "Success"),
                Some("critical") => push_attr(&mut x, "hint-buttonStyle", "Critical"),
                _ => {}
            }
            x.push_str("/>");
            x
        })
        .collect();
    (buttons, dropped)
}

fn resolve_inputs(defs: &[InputDef], lang: Language) -> (Vec<String>, usize) {
    let usable: Vec<&InputDef> = defs
        .iter()
        .filter(|d| !d.id.trim().is_empty())
        .filter(|d| d.kind.as_deref() != Some("selection") || !d.choices.is_empty())
        .collect();
    let dropped = defs.len() - usable.len().min(MAX_INPUTS);
    let xml = usable
        .into_iter()
        .take(MAX_INPUTS)
        .map(|d| {
            let selection = d.kind.as_deref() == Some("selection");
            let mut x = String::from("<input");
            push_attr(&mut x, "id", &d.id);
            push_attr(&mut x, "type", if selection { "selection" } else { "text" });
            if let Some(title) = pick(d.title.as_ref(), lang) {
                push_attr(&mut x, "title", &title);
            }
            if !selection && let Some(ph) = pick(d.placeholder.as_ref(), lang) {
                push_attr(&mut x, "placeHolderContent", &ph);
            }
            if let Some(default) = &d.default {
                push_attr(&mut x, "defaultInput", default);
            }
            if selection {
                x.push('>');
                for c in d.choices.iter().take(MAX_CHOICES) {
                    let label = pick(Some(&c.label), lang).unwrap_or_else(|| c.id.clone());
                    x.push_str("<selection");
                    push_attr(&mut x, "id", &c.id);
                    push_attr(&mut x, "content", &label);
                    x.push_str("/>");
                }
                x.push_str("</input>");
            } else {
                x.push_str("/>");
            }
            x
        })
        .collect();
    (xml, dropped)
}

fn progress_xml(
    p: &Progress,
    lang: Language,
    bind: &mut dyn FnMut(&str, String) -> String,
) -> String {
    let value = match p.value.as_ref().and_then(|v| v.as_f64()) {
        Some(f) if f.is_finite() => format!("{:.3}", f.clamp(0.0, 1.0)),
        _ => "indeterminate".to_string(),
    };
    // `status` is required by the schema; an ellipsis beats a rejected toast.
    let status = pick(Some(&p.status), lang).unwrap_or_else(|| "…".to_string());
    let mut x = String::from("<progress");
    push_attr(&mut x, "value", &bind("progressValue", value));
    push_attr(&mut x, "status", &bind("progressStatus", status));
    if let Some(title) = pick(p.title.as_ref(), lang) {
        push_attr(&mut x, "title", &bind("progressTitle", title));
    }
    if let Some(label) = p.label.as_deref().filter(|l| !l.trim().is_empty()) {
        push_attr(
            &mut x,
            "valueStringOverride",
            &bind("progressLabel", label.into()),
        );
    }
    x.push_str("/>");
    x
}

/// A plain informational toast from newsflash itself (startup problems,
/// the install confirmation) — no buttons, nothing to answer.
pub fn notice_xml(title: &str, body: &str) -> String {
    format!(
        r#"<toast duration="long"><visual><binding template="ToastGeneric"><text hint-maxLines="2">{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    )
}

/// Where a click came from, carried in the button's `arguments` so that
/// even a freshly COM-launched process (the daemon was not running when
/// Kenny clicked an old toast in Notification Center) can publish the
/// action_result without any state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activation {
    Button {
        action_id: String,
        envelope_id: String,
        ack_id: Option<String>,
        hub_id: String,
        demo: bool,
    },
    /// The toast body itself (no `click_url`) — logged, nothing published.
    Body { hub_id: String },
    /// The Notification Center group header — ignored.
    Header,
}

pub fn encode_activation(a: &Activation) -> String {
    let pairs: Vec<(&str, &str)> = match a {
        Activation::Button {
            action_id,
            envelope_id,
            ack_id,
            hub_id,
            demo,
        } => {
            let mut p = vec![
                ("k", "button"),
                ("a", action_id.as_str()),
                ("e", envelope_id.as_str()),
                ("h", hub_id.as_str()),
            ];
            if let Some(ack) = ack_id {
                p.push(("c", ack.as_str()));
            }
            if *demo {
                p.push(("d", "1"));
            }
            p
        }
        Activation::Body { hub_id } => vec![("k", "body"), ("h", hub_id.as_str())],
        Activation::Header => vec![("k", "header")],
    };
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `None` = not ours (or mangled) — the caller logs and ignores it.
pub fn decode_activation(args: &str) -> Option<Activation> {
    let mut get = std::collections::HashMap::new();
    for pair in args.split('&') {
        let (k, v) = pair.split_once('=')?;
        get.insert(k, percent_decode(v)?);
    }
    match get.get("k")?.as_str() {
        "button" => Some(Activation::Button {
            action_id: get.get("a").filter(|a| !a.is_empty())?.clone(),
            envelope_id: get.get("e")?.clone(),
            ack_id: get.get("c").cloned(),
            hub_id: get.get("h")?.clone(),
            demo: get.get("d").is_some_and(|d| d == "1"),
        }),
        "body" => Some(Activation::Body {
            hub_id: get.get("h")?.clone(),
        }),
        "header" => Some(Activation::Header),
        _ => None,
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// feat-win-7: tags longer than the platform cap keep a readable prefix plus a
/// hash of the whole tag, so two long tags never collide by truncation.
fn clamp_tag(tag: &str) -> String {
    if tag.encode_utf16().count() <= MAX_TAG_UNITS {
        return tag.to_string();
    }
    let hash = format!("{:016x}", fnv1a(tag.as_bytes()));
    let budget = MAX_TAG_UNITS - hash.len() - 1;
    let mut prefix = String::new();
    let mut units = 0;
    for c in tag.chars() {
        units += c.len_utf16();
        if units > budget {
            break;
        }
        prefix.push(c);
    }
    format!("{prefix}-{hash}")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// XML text/attribute escaping, and removal of the control characters
/// XML 1.0 forbids — otherwise `LoadXml` rejects the toast, the render
/// fails and the message would nack-loop on a stray byte.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn push_attr(xml: &mut String, name: &str, value: &str) {
    xml.push(' ');
    xml.push_str(name);
    xml.push_str("=\"");
    xml.push_str(&escape(value));
    xml.push('"');
}

/// Unix seconds → `YYYY-MM-DDTHH:MM:SSZ` (the toast's
/// `displayTimestamp`: when the message was published, not when this
/// OS happened to boot and fetch it).
pub fn rfc3339_utc(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::parse_envelope;

    fn env(json: &str) -> Envelope {
        parse_envelope(json.as_bytes()).unwrap()
    }

    fn input() -> BuildInput<'static> {
        BuildInput {
            hub_id: "hub-1",
            published_at_ms: 1_756_400_000_000,
            language: Language::Nl,
            critical_scenario: CriticalScenario::Reminder,
            logo_uri: None,
            hero_uri: None,
            silent: false,
            demo: false,
            lifetimes: Lifetimes {
                ephemeral: 10,
                info: None,
                warning: None,
                critical: None,
            },
            popup: PopupDurations::default(),
            link_base: None,
        }
    }

    fn build(json: &str) -> WinToast {
        build_toast(&env(json), &input())
    }

    #[test]
    fn feat_8_a_deferred_toast_is_silent_and_suppresses_its_popup() {
        let quiet = build(r#"{"v":1,"id":"x","gate_outcome":"deferred","title":{"nl":"a"}}"#);
        assert!(quiet.suppress_popup);
        assert!(quiet.xml.contains(r#"<audio silent="true"/>"#));
        let live = build(r#"{"v":1,"id":"x","gate_outcome":"live","title":{"nl":"a"}}"#);
        assert!(!live.suppress_popup);
        assert!(!live.xml.contains("silent"));
    }

    #[test]
    fn feat_7_configured_popup_durations_pick_short_or_long() {
        assert_eq!(windows_duration(10_000), "short");
        assert_eq!(windows_duration(30_000), "long");
        let e = env(r#"{"v":1,"id":"x","priority":"info","title":{"nl":"a"}}"#);
        let mut input = input();
        input.popup.info_ms = 60_000;
        assert!(build_toast(&e, &input).xml.contains(r#"duration="long""#));
    }

    #[test]
    fn w1_priorities_map_to_duration_and_scenario() {
        let info = build(r#"{"v":1,"id":"x","priority":"info","title":{"nl":"a"}}"#);
        assert!(info.xml.contains(r#"duration="short""#));
        assert!(!info.xml.contains("scenario="));
        let warn = build(r#"{"v":1,"id":"x","priority":"warning","title":{"nl":"a"}}"#);
        assert!(warn.xml.contains(r#"duration="long""#));
        let crit = build(r#"{"v":1,"id":"x","priority":"critical","title":{"nl":"a"}}"#);
        assert!(crit.xml.contains(r#"scenario="reminder""#));
        assert!(!crit.xml.contains("duration="));
        let unknown = build(r#"{"v":1,"id":"x","priority":"shouting","title":{"nl":"a"}}"#);
        assert!(unknown.xml.contains(r#"duration="short""#));
    }

    #[test]
    fn w1_the_critical_scenario_is_configurable() {
        let mut i = input();
        i.critical_scenario = CriticalScenario::Urgent;
        let t = build_toast(
            &env(r#"{"v":1,"id":"x","priority":"critical","title":{"nl":"a"}}"#),
            &i,
        );
        assert!(t.xml.contains(r#"scenario="urgent""#));
        assert_eq!(
            CriticalScenario::parse("alarm"),
            Some(CriticalScenario::Alarm)
        );
        assert_eq!(CriticalScenario::parse("loud"), None);
    }

    #[test]
    fn w1_the_logo_follows_the_priority() {
        assert_eq!(logo_asset(Some("critical")), "critical.png");
        assert_eq!(logo_asset(Some("warning")), "warning.png");
        assert_eq!(logo_asset(None), "info.png");
    }

    #[test]
    fn m3_texts_use_the_shared_language_pick_and_are_xml_escaped_not_markup_escaped() {
        let t = build(
            r#"{"v":1,"id":"x","title":{"nl":"5 < 7 & \"zo\"","en":"no"},"message":{"en":"<b>bold</b>"}}"#,
        );
        assert!(t.xml.contains("5 &lt; 7 &amp; &quot;zo&quot;"));
        assert!(t.xml.contains("<text>&lt;b&gt;bold&lt;/b&gt;</text>"));
    }

    #[test]
    fn w_control_characters_are_stripped_so_loadxml_never_rejects_the_toast() {
        let t = build("{\"v\":1,\"id\":\"x\",\"title\":{\"nl\":\"a\\u0000b\\u0007c\\nd\"}}");
        assert!(t.xml.contains("abc\nd"));
    }

    #[test]
    fn k12_default_pair_on_windows_too() {
        let t = build(r#"{"v":1,"id":"p-1","title":{"nl":"a"}}"#);
        assert!(t.xml.contains(r#"content="Gelezen""#));
        assert!(t.xml.contains(r#"content="Snooze""#));
        assert!(
            t.xml
                .contains("k=button&amp;a=gelezen&amp;e=p-1&amp;h=hub-1")
        );
    }

    #[test]
    fn w2_up_to_five_buttons_then_truncate_and_report() {
        let actions: Vec<String> = (0..7)
            .map(|i| format!(r#"{{"id":"b{i}","label":{{"nl":"B{i}"}}}}"#))
            .collect();
        let t = build(&format!(
            r#"{{"v":1,"id":"x","title":{{"nl":"a"}},"actions":[{}]}}"#,
            actions.join(",")
        ));
        assert_eq!(t.xml.matches("<action ").count(), MAX_BUTTONS);
        assert_eq!(t.dropped_actions, 2);
        assert!(t.xml.contains("B4") && !t.xml.contains("B5"));
    }

    #[test]
    fn w3_styled_buttons_switch_on_use_button_style() {
        let t = build(
            r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[
                {"id":"yes","label":{"nl":"Ja"},"style":"success"},
                {"id":"no","label":{"nl":"Nee"},"style":"critical"},
                {"id":"meh","label":{"nl":"Later"},"style":"purple"}]}"#,
        );
        assert!(t.xml.contains(r#"useButtonStyle="true""#));
        assert!(t.xml.contains(r#"hint-buttonStyle="Success""#));
        assert!(t.xml.contains(r#"hint-buttonStyle="Critical""#));
        assert_eq!(t.xml.matches("hint-buttonStyle").count(), 2);
        let plain = build(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#);
        assert!(!plain.xml.contains("useButtonStyle"));
    }

    #[test]
    fn w4_a_url_button_opens_the_link_and_only_web_links_qualify() {
        let t = build(
            r#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[
                {"id":"cam","label":{"nl":"Camera"},"url":"http://ha.local:8123/cam"},
                {"id":"evil","label":{"nl":"Evil"},"url":"file:///C:/Windows/System32/calc.exe"}]}"#,
        );
        assert!(t.xml.contains(
            r#"content="Camera" activationType="protocol" arguments="http://ha.local:8123/cam""#
        ));
        // The non-web url degrades to an ordinary reply button.
        assert!(t.xml.contains("a=evil"));
        assert!(!t.xml.contains("file:///"));
        assert!(!is_openable_url("ms-settings:privacy"));
        assert!(!is_openable_url("http://"));
        assert!(!is_openable_url("http://a b"));
        assert!(is_openable_url("HTTPS://example.org/x?y=1"));
    }

    #[test]
    fn w5_click_url_becomes_a_protocol_launch() {
        let t = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"click_url":"http://ha.local/"}"#);
        assert!(
            t.xml
                .starts_with(r#"<toast launch="http://ha.local/" activationType="protocol""#)
        );
        let bad = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"click_url":"javascript:alert(1)"}"#);
        assert!(bad.xml.starts_with(r#"<toast launch="k=body&amp;h=hub-1""#));
    }

    #[test]
    fn w6_images_come_from_the_shell_resolved_uris() {
        let mut i = input();
        i.logo_uri = Some("file:///C:/nf/assets/info.png");
        i.hero_uri = Some("file:///C:/nf/images/hub-1.jpg");
        let t = build_toast(&env(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#), &i);
        assert!(t.xml.contains(
            r#"<image placement="appLogoOverride" src="file:///C:/nf/assets/info.png"/>"#
        ));
        assert!(
            t.xml
                .contains(r#"<image placement="hero" src="file:///C:/nf/images/hub-1.jpg"/>"#)
        );
    }

    #[test]
    fn w7_tags_pass_through_and_long_tags_are_clamped_without_collisions() {
        let t = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"tag":"wasmachine"}"#);
        assert_eq!(t.tag.as_deref(), Some("wasmachine"));
        let a = clamp_tag(&"x".repeat(100));
        let b = clamp_tag(&format!("{}y", "x".repeat(99)));
        assert_eq!(a.encode_utf16().count(), MAX_TAG_UNITS);
        assert_ne!(a, b);
        let none = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"tag":"  "}"#);
        assert_eq!(none.tag, None);
    }

    #[test]
    fn w8_progress_values_are_clamped_and_bad_ones_read_indeterminate() {
        let t = build(
            r#"{"v":1,"id":"x","title":{"nl":"a"},"progress":{"value":1.7,"status":{"nl":"Wassen"},"label":"3/5"}}"#,
        );
        assert!(
            t.xml
                .contains(r#"<progress value="1.000" status="Wassen" valueStringOverride="3/5"/>"#)
        );
        let ind = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"progress":{"value":"soon"}}"#);
        assert!(
            ind.xml
                .contains(r#"<progress value="indeterminate" status="…"/>"#)
        );
    }

    #[test]
    fn w8_tag_plus_progress_is_a_data_bound_live_toast() {
        let t = build(
            r#"{"v":1,"id":"x","tag":"wasmachine","title":{"nl":"Wasmachine"},
                "progress":{"value":0.25,"status":{"nl":"Wassen"}}}"#,
        );
        assert!(
            t.xml
                .contains(r#"<text hint-maxLines="2">{title}</text><text>{body}</text>"#)
        );
        assert!(
            t.xml
                .contains(r#"<progress value="{progressValue}" status="{progressStatus}"/>"#)
        );
        assert_eq!(
            t.data,
            vec![
                ("title".to_string(), "Wasmachine".to_string()),
                ("body".to_string(), String::new()),
                ("progressValue".to_string(), "0.250".to_string()),
                ("progressStatus".to_string(), "Wassen".to_string()),
            ]
        );
        // Without a tag there is nothing to update: plain inline values.
        let plain = build(r#"{"v":1,"id":"x","title":{"nl":"a"},"progress":{"value":0.5}}"#);
        assert!(plain.data.is_empty());
        assert!(plain.xml.contains(r#"value="0.500""#));
    }

    #[test]
    fn w9_text_and_selection_inputs_render_in_the_actions_block() {
        let t = build(
            r#"{"v":1,"id":"x","title":{"nl":"a"},"inputs":[
                {"id":"reply","placeholder":{"nl":"Antwoord…"}},
                {"id":"snooze_minutes","type":"selection","default":"60","choices":[
                    {"id":"5","label":{"nl":"5 min"}},{"id":"60","label":{"nl":"1 uur"}}]},
                {"id":"empty","type":"selection"}]}"#,
        );
        assert!(
            t.xml
                .contains(r#"<input id="reply" type="text" placeHolderContent="Antwoord…"/>"#)
        );
        assert!(t.xml.contains(
            r#"<input id="snooze_minutes" type="selection" defaultInput="60"><selection id="5" content="5 min"/><selection id="60" content="1 uur"/></input>"#
        ));
        assert_eq!(
            t.dropped_inputs, 1,
            "a selection without choices is dropped"
        );
        let actions = t.xml.find("<actions>").unwrap();
        assert!(t.xml.find("<input").unwrap() > actions);
    }

    #[test]
    fn w10_source_becomes_header_and_attribution() {
        let t = build(r#"{"v":1,"id":"x","source":"home-assistant","title":{"nl":"a"}}"#);
        assert!(t.xml.contains(
            r#"<header id="home-assistant" title="home-assistant" arguments="k=header"/>"#
        ));
        assert!(
            t.xml
                .contains(r#"<text placement="attribution">via home-assistant</text>"#)
        );
        let anon = build(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#);
        assert!(!anon.xml.contains("<header"));
    }

    #[test]
    fn w11_the_timestamp_is_the_publish_time() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_756_400_000), "2025-08-28T16:53:20Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        let t = build(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#);
        assert!(t.xml.contains(r#"displayTimestamp="2025-08-28T16:53:20Z""#));
    }

    #[test]
    fn k6_a_configured_chime_silences_the_toast() {
        let mut i = input();
        i.silent = true;
        let t = build_toast(&env(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#), &i);
        assert!(t.xml.ends_with(r#"<audio silent="true"/></toast>"#));
    }

    #[test]
    fn a_notice_is_escaped_and_has_no_buttons() {
        let x = notice_xml("newsflash <stopped>", "fix & restart");
        assert!(x.contains("newsflash &lt;stopped&gt;") && x.contains("fix &amp; restart"));
        assert!(!x.contains("<actions>"));
    }

    #[test]
    fn w12_ephemeral_and_config_lifetimes_become_the_expiration_time() {
        let published = input().published_at_ms;
        let mut i = input();
        let plain = build_toast(&env(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#), &i);
        assert_eq!(
            plain.expires_at_ms, None,
            "nothing set: Windows' own 3 days"
        );
        let eph = build_toast(
            &env(r#"{"v":1,"id":"x","ephemeral":true,"title":{"nl":"a"}}"#),
            &i,
        );
        assert_eq!(eph.expires_at_ms, Some(published + 10 * 60_000));
        i.lifetimes.info = Some(60);
        let info = build_toast(&env(r#"{"v":1,"id":"x","title":{"nl":"a"}}"#), &i);
        assert_eq!(info.expires_at_ms, Some(published + 60 * 60_000));
    }

    #[test]
    fn live_messages_get_their_buttons_and_path_links_resolve_against_the_base() {
        let live = r#"{"v":1,"id":"p","title":{"nl":"2 planten"},"click_url":"/control-panel/plants",
            "data":{"action_buttons":[{"action":"PLANTCARE_WATER_z","title":"Water gegeven"},
            {"action":"PLANTCARE_SNOOZE_z","title":"Snooze 3 dagen"},
            {"action":"PLANTCARE_SKIP_z","title":"Nog te nat"}]}}"#;
        let t = build(live);
        assert_eq!(t.xml.matches("<action ").count(), 3);
        assert!(t.xml.contains(r#"content="Nog te nat""#));
        assert!(t.xml.contains("a=PLANTCARE_WATER_z"));
        assert!(
            !t.xml.contains("control-panel"),
            "no base: the path link is dropped"
        );
        let mut i = input();
        i.link_base = Some("http://10.10.10.2:8123");
        let t = build_toast(&env(live), &i);
        assert!(t.xml.starts_with(
            r#"<toast launch="http://10.10.10.2:8123/control-panel/plants" activationType="protocol""#
        ));
    }

    #[test]
    fn w_activation_args_round_trip_including_awkward_ids() {
        let a = Activation::Button {
            action_id: "ik pak&het=op".into(),
            envelope_id: "01J/é".into(),
            ack_id: Some("ctx-7".into()),
            hub_id: "42".into(),
            demo: true,
        };
        assert_eq!(decode_activation(&encode_activation(&a)), Some(a));
        let b = Activation::Body { hub_id: "7".into() };
        assert_eq!(decode_activation(&encode_activation(&b)), Some(b));
        assert_eq!(
            decode_activation(&encode_activation(&Activation::Header)),
            Some(Activation::Header)
        );
        assert_eq!(decode_activation("k=button&e=x&h=1"), None, "no action id");
        assert_eq!(decode_activation("http://ha.local/"), None);
        assert_eq!(decode_activation("k=button&a=%ZZ&e=x&h=1"), None);
    }
}
