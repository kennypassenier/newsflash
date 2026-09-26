//! The chime ships inside the binary (feat-9, Kenny 2026-09-26: soft
//! pulse), so an install on either OS leaves nothing to copy by hand.
//! The installers write it next to newsflash's data and point
//! `sound_file` at it, unless the config already names a sound file:
//! a choice Kenny made in the config always wins.

use crate::config_edit;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "newsflash-soft-pulse.wav";
pub const SOFT_PULSE: &[u8] = include_bytes!("../../assets/chimes/newsflash-soft-pulse.wav");

pub enum Outcome {
    /// `sound_file` was unset; it now points at the shipped chime.
    Configured(PathBuf),
    /// The config already names a sound file; the chime file was still
    /// refreshed, the config left alone.
    KeptExisting(String),
}

/// Writes the chime into `data_dir/chimes/` and sets `sound_file` in
/// `config` when it has none. `config` must exist.
pub fn ensure(data_dir: &Path, config: &Path) -> Result<Outcome, String> {
    let dir = data_dir.join("chimes");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let wav = dir.join(FILE_NAME);
    if std::fs::read(&wav).ok().as_deref() != Some(SOFT_PULSE) {
        let tmp = dir.join(format!(".{FILE_NAME}.new"));
        std::fs::write(&tmp, SOFT_PULSE)
            .and_then(|_| std::fs::rename(&tmp, &wav))
            .map_err(|e| format!("writing {}: {e}", wav.display()))?;
    }
    let text = std::fs::read_to_string(config)
        .map_err(|e| format!("reading {}: {e}", config.display()))?;
    if let Some(existing) = config_edit::get_string(&text, "sound_file") {
        return Ok(Outcome::KeptExisting(existing));
    }
    let updated = config_edit::set_string(&text, "sound_file", &wav.display().to_string());
    let tmp = config.with_extension("toml.new");
    std::fs::write(&tmp, updated)
        .and_then(|_| std::fs::rename(&tmp, config))
        .map_err(|e| format!("updating {}: {e}", config.display()))?;
    Ok(Outcome::Configured(wav))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("newsflash-chime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn feat_9_an_unset_sound_file_gets_the_shipped_chime() {
        let d = dir("unset");
        let config = d.join("config.toml");
        std::fs::write(
            &config,
            "hub_url = \"http://h:1\"\n#sound_file = \"x.wav\"\n",
        )
        .unwrap();
        let Outcome::Configured(wav) = ensure(&d, &config).unwrap() else {
            panic!("expected the chime to be configured")
        };
        assert_eq!(std::fs::read(&wav).unwrap(), SOFT_PULSE);
        let text = std::fs::read_to_string(&config).unwrap();
        assert_eq!(
            config_edit::get_string(&text, "sound_file").as_deref(),
            Some(wav.to_str().unwrap())
        );
        assert!(text.contains("hub_url = \"http://h:1\""), "{text}");
    }

    #[test]
    fn feat_9_a_sound_file_already_chosen_is_left_alone() {
        let d = dir("kept");
        let config = d.join("config.toml");
        let before = "hub_url = \"http://h:1\"\nsound_file = \"/mine.wav\"\n";
        std::fs::write(&config, before).unwrap();
        assert!(matches!(
            ensure(&d, &config).unwrap(),
            Outcome::KeptExisting(s) if s == "/mine.wav"
        ));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
    }
}
