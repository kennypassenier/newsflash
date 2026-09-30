//! Config load + validation (M2, AR10, AR17). Every rejection names
//! the field and the remedy; the token never comes from the config
//! file itself.

use courier_core::toast::{
    Language, Lifetimes, MAX_LIFETIME_MINUTES, MAX_POPUP_SECONDS, PopupDurations, resolve_link,
};
use std::path::{Path, PathBuf};

pub const DEFAULT_TTL_MINUTES: u64 = 10;
/// AR19: poll wait window (seconds). Short enough that a SIGTERM mid
/// long-poll settles well inside the unit's stop timeout (AR20).
pub const POLL_WAIT_SECS: u64 = 20;
/// M10 default margin (standing rule 27 — this margin is the knob; the
/// "no cap for a persistent toast" branch stays pinned, see
/// `courier_core::toast::interactive_wait_cap_ms`). Generous headroom
/// above a bounded toast's own expiry for the watcher thread's safety
/// cap — not a UX duration, just "give up waiting eventually".
pub const DEFAULT_INTERACTIVE_WAIT_MARGIN_MS: u32 = 30_000;

#[derive(Debug)]
pub struct Config {
    /// Without trailing slash, plain http (the hub is LAN-only, N3).
    pub hub_url: String,
    pub topic: String,
    pub subscription: String,
    pub language: Language,
    pub ttl_ms: u64,
    pub sound_file: Option<PathBuf>,
    pub token: String,
    pub interactive_wait_margin_ms: u32,
    /// How long Snooze hides a notification before it shows again (feat-11).
    pub snooze_minutes: u32,
    /// Where snoozed notifications wait (feat-11); default `<topic>.snooze`.
    pub snooze_topic: String,
    /// How long notifications may exist (ephemeral + per priority).
    pub lifetimes: Lifetimes,
    /// How long info and warning popups stay on screen (feat-7).
    pub popup: PopupDurations,
    /// Base for path-only links (`/control-panel/homelab` → Home
    /// Assistant). `None` = such links are not offered.
    pub link_base_url: Option<String>,
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });
    base.join("newsflash").join("config.toml")
}

#[derive(serde::Deserialize)]
struct RawConfig {
    hub_url: Option<String>,
    topic: Option<String>,
    subscription: Option<String>,
    language: Option<String>,
    ttl_minutes: Option<u64>,
    interactive_wait_margin_ms: Option<u32>,
    snooze_minutes: Option<u32>,
    snooze_topic: Option<String>,
    sound_file: Option<String>,
    token: Option<String>,
    token_file: Option<String>,
    ephemeral_minutes: Option<u32>,
    expire_info_minutes: Option<u32>,
    expire_warning_minutes: Option<u32>,
    expire_critical_minutes: Option<u32>,
    popup_info_seconds: Option<u32>,
    popup_warning_seconds: Option<u32>,
    link_base_url: Option<String>,
}

/// Errors are full sentences with remedies (standing rule 11) — they
/// are the startup output Kenny reads when the unit refuses to start.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "cannot read config {}: {e}. Copy config.example.toml there and fill in hub_url.",
            path.display()
        )
    })?;
    let raw: RawConfig = toml::from_str(&text)
        .map_err(|e| format!("config {} is not valid TOML: {e}", path.display()))?;

    if raw.token.is_some() {
        return Err(
            "config contains an inline token. Secrets never live in the config file (AR10): \
             remove the token key and provide KYU_TOKEN via latch run, or point token_file \
             at a 0600 file."
                .to_string(),
        );
    }

    let hub_url = raw
        .hub_url
        .ok_or("config misses hub_url. Set it, e.g. hub_url = \"http://127.0.0.1:8080\".")?;
    let hub_url = hub_url.trim().trim_end_matches('/').to_string();
    if !hub_url.starts_with("http://") {
        return Err(format!(
            "hub_url {hub_url:?} must start with http:// — the hub speaks plain HTTP on the LAN \
             (kyu N3); newsflash deliberately ships no TLS stack (AR2)."
        ));
    }

    let language = match raw.language.as_deref().unwrap_or("nl") {
        "nl" => Language::Nl,
        "en" => Language::En,
        other => {
            return Err(format!(
                "language {other:?} is not supported. Use \"nl\" or \"en\"."
            ));
        }
    };

    let ttl_minutes = raw.ttl_minutes.unwrap_or(DEFAULT_TTL_MINUTES);
    if ttl_minutes == 0 {
        return Err(
            "ttl_minutes = 0 would expire every message instantly. Use 1 or higher \
             (default 10, per SCOPE S4)."
                .to_string(),
        );
    }
    // A year is already absurd for a toast; anything larger risks
    // arithmetic overflow downstream (hardening gap G16).
    if ttl_minutes > 525_600 {
        return Err(format!(
            "ttl_minutes = {ttl_minutes} is longer than a year. Use something sane \
             (default 10, per SCOPE S4)."
        ));
    }

    let topic = raw.topic.unwrap_or_else(|| "notify.kenny".to_string());
    let subscription = raw.subscription.unwrap_or_else(|| "desktop".to_string());
    for (field, value) in [("topic", &topic), ("subscription", &subscription)] {
        if value.trim().is_empty() {
            return Err(format!(
                "{field} is empty. Remove the key to use the default, or set a real name."
            ));
        }
    }

    let sound_file = match raw.sound_file {
        None => None,
        Some(p) => {
            let pb = PathBuf::from(&p);
            if !pb.is_file() {
                return Err(format!(
                    "sound_file {p:?} does not exist. Point it at a playable file \
                     (house style: cyberpunk-soft, never metallic) or remove the key for silence."
                ));
            }
            Some(pb)
        }
    };

    let token = resolve_token(raw.token_file.as_deref())?;

    let snooze_minutes = match raw.snooze_minutes {
        None => crate::snooze::DEFAULT_SNOOZE_MINUTES,
        Some(m) if (1..=crate::snooze::MAX_SNOOZE_MINUTES).contains(&m) => m,
        Some(m) => {
            return Err(format!(
                "snooze_minutes = {m} is out of range. Use 1 to {} minutes, or remove the key \
                 for the default ({}).",
                crate::snooze::MAX_SNOOZE_MINUTES,
                crate::snooze::DEFAULT_SNOOZE_MINUTES
            ));
        }
    };
    let snooze_topic = raw
        .snooze_topic
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| crate::snooze::default_topic(&topic));
    if snooze_topic == topic {
        return Err(format!(
            "snooze_topic {snooze_topic:?} is the subscribed topic itself. A snoozed \
             notification would reach every subscriber of it again (the phone, the lights); \
             remove the key for the default ({}).",
            crate::snooze::default_topic(&topic)
        ));
    }
    let interactive_wait_margin_ms = raw
        .interactive_wait_margin_ms
        .unwrap_or(DEFAULT_INTERACTIVE_WAIT_MARGIN_MS);

    let minutes = |key: &str, v: Option<u32>| -> Result<Option<u32>, String> {
        match v {
            Some(m) if !(1..=MAX_LIFETIME_MINUTES).contains(&m) => Err(format!(
                "{key} = {m} is out of range. Use 1 to {MAX_LIFETIME_MINUTES} minutes \
                 (Windows keeps a notification 3 days at most), or remove the key."
            )),
            other => Ok(other),
        }
    };
    let lifetimes = Lifetimes {
        ephemeral: minutes("ephemeral_minutes", raw.ephemeral_minutes)?
            .unwrap_or(courier_core::toast::DEFAULT_EPHEMERAL_MINUTES),
        info: minutes("expire_info_minutes", raw.expire_info_minutes)?,
        warning: minutes("expire_warning_minutes", raw.expire_warning_minutes)?,
        critical: minutes("expire_critical_minutes", raw.expire_critical_minutes)?,
    };
    let seconds = |key: &str, v: Option<u32>, default: u32| -> Result<u32, String> {
        match v {
            Some(s) if !(1..=MAX_POPUP_SECONDS).contains(&s) => Err(format!(
                "{key} = {s} is out of range. Use 1 to {MAX_POPUP_SECONDS} seconds, or remove \
                 the key for the default ({default} s). Critical popups always stay until \
                 answered and have no such key."
            )),
            Some(s) => Ok(s),
            None => Ok(default),
        }
    };
    let popup = PopupDurations {
        info_ms: seconds(
            "popup_info_seconds",
            raw.popup_info_seconds,
            courier_core::toast::DEFAULT_POPUP_INFO_SECONDS,
        )? * 1000,
        warning_ms: seconds(
            "popup_warning_seconds",
            raw.popup_warning_seconds,
            courier_core::toast::DEFAULT_POPUP_WARNING_SECONDS,
        )? * 1000,
    };
    let link_base_url = match raw.link_base_url {
        None => None,
        Some(u) if resolve_link(&u, None).is_some() => Some(u.trim_end_matches('/').to_string()),
        Some(u) => {
            return Err(format!(
                "link_base_url {u:?} is not a full http(s) address. Set it to where \
                 path-only links live (Home Assistant: \"http://10.10.10.2:8123\"), or remove it."
            ));
        }
    };

    Ok(Config {
        hub_url,
        topic,
        subscription,
        language,
        ttl_ms: ttl_minutes * 60_000,
        sound_file,
        token,
        interactive_wait_margin_ms,
        snooze_minutes,
        snooze_topic,
        lifetimes,
        popup,
        link_base_url,
    })
}

/// AR10 order: KYU_TOKEN env (latch-injected) first, then
/// token_file (0600, checked once — deliberately no TOCTOU hardening
/// on a single-admin machine).
fn resolve_token(token_file: Option<&str>) -> Result<String, String> {
    match std::env::var("KYU_TOKEN") {
        Ok(v) if v.trim().is_empty() => {
            return Err(
                "KYU_TOKEN is set but empty. Fix the latch secret or unset the variable \
                 to fall back to token_file."
                    .to_string(),
            );
        }
        Ok(v) => return Ok(v.trim().to_string()),
        Err(_) => {}
    }
    let Some(path) = token_file else {
        return Err(
            "no token available. Set KYU_TOKEN (latch run -- newsflash) or set \
             token_file in the config to a 0600 file holding the app token from the hub's \
             /apps page."
                .to_string(),
        );
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta =
            std::fs::metadata(path).map_err(|e| format!("cannot stat token_file {path:?}: {e}"))?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "token_file {path:?} is readable by group/others. Run: chmod 600 {path}"
            ));
        }
    }
    let token = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read token_file {path:?}: {e}"))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(format!(
            "token_file {path:?} is empty. Put the app token from the hub's /apps page in it."
        ));
    }
    Ok(token.to_string())
}
