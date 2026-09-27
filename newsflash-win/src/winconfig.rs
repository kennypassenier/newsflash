//! The Windows-only config keys. The shared keys (hub_url, topic,
//! subscription, language, ttl_minutes, sound_file, token_file, …) are
//! loaded by `newsflash::config::load` exactly as on Linux, which
//! ignores unknown keys — so one config format serves both OSes and the
//! keys below are simply invisible to the Linux binary.

use courier_core::wintoast::CriticalScenario;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinConfig {
    pub critical_scenario: CriticalScenario,
}

#[derive(serde::Deserialize)]
struct Raw {
    critical_scenario: Option<String>,
    sound_file: Option<String>,
}

/// Same contract as the shared loader: every rejection names the field
/// and the remedy (standing rule 11).
pub fn load(path: &Path) -> Result<WinConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<WinConfig, String> {
    let raw: Raw = toml::from_str(text).map_err(|e| format!("config is not valid TOML: {e}"))?;
    let critical_scenario = match raw.critical_scenario.as_deref() {
        None => CriticalScenario::Reminder,
        Some(s) => CriticalScenario::parse(s).ok_or_else(|| {
            format!(
                "critical_scenario {s:?} is not supported. Use \"reminder\" (stays on screen \
                 until answered, the default), \"urgent\" (breaks through Do Not Disturb) or \
                 \"alarm\" (reminder + looping sound)."
            )
        })?,
    };
    // Windows plays the chime with PlaySound, which only speaks WAV.
    if let Some(sound) = raw.sound_file.as_deref()
        && !sound.to_ascii_lowercase().ends_with(".wav")
    {
        return Err(format!(
            "sound_file {sound:?} is not a .wav file. Windows plays the chime with PlaySound, \
             which only supports WAV — convert it, or remove the key for silence."
        ));
    }
    Ok(WinConfig { critical_scenario })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feat_win_1_critical_scenario_defaults_to_reminder() {
        let c = parse(r#"hub_url = "http://x""#).unwrap();
        assert_eq!(c.critical_scenario, CriticalScenario::Reminder);
        let c = parse(r#"critical_scenario = "urgent""#).unwrap();
        assert_eq!(c.critical_scenario, CriticalScenario::Urgent);
    }

    #[test]
    fn m2_bad_windows_keys_name_the_field_and_remedy() {
        let e = parse(r#"critical_scenario = "loud""#).unwrap_err();
        assert!(e.contains("critical_scenario") && e.contains("reminder"));
        let e = parse(r#"sound_file = "C:\\chime.ogg""#).unwrap_err();
        assert!(e.contains("sound_file") && e.contains("WAV"));
        assert!(parse(r#"sound_file = "C:\\chime.WAV""#).is_ok());
    }
}
