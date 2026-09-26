//! Where things live on Windows — the XDG paths' counterparts.
//!
//! - config + DPAPI token: `%APPDATA%\newsflash\` (roams with the profile)
//! - dedup store, log, assets, image cache: `%LOCALAPPDATA%\newsflash\`
//! - installed binaries: `%LOCALAPPDATA%\Programs\newsflash\`
//!
//! Everything is resolved from the environment so tests can point it
//! anywhere; a missing variable falls back to the current directory
//! rather than panicking (the caller's error then names the path).

use std::path::PathBuf;

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    env_dir("APPDATA").join("newsflash")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// DPAPI-encrypted app token (`newsflash set-token`), bound to this
/// Windows user account.
pub fn token_path() -> PathBuf {
    config_dir().join("token.dpapi")
}

pub fn data_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("newsflash")
}

pub fn state_path() -> PathBuf {
    data_dir().join("seen.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("newsflash.log")
}

pub fn assets_dir() -> PathBuf {
    data_dir().join("assets")
}

pub fn images_dir() -> PathBuf {
    data_dir().join("images")
}

pub fn install_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Programs").join("newsflash")
}

/// Same directory as far as Windows is concerned: case-insensitive,
/// trailing backslashes and surrounding quotes/space ignored.
fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// The user PATH with `dir` appended, or `None` when it is already
/// there. Existing entries are kept verbatim (`%VARS%` unexpanded).
pub fn path_with(current: &str, dir: &str) -> Option<String> {
    if current.split(';').any(|e| same_dir(e, dir)) {
        return None;
    }
    let base = current.trim_end_matches(';');
    Some(if base.is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    })
}

/// The user PATH without `dir`, or `None` when it was not there.
pub fn path_without(current: &str, dir: &str) -> Option<String> {
    let entries: Vec<&str> = current.split(';').collect();
    if !entries.iter().any(|e| same_dir(e, dir)) {
        return None;
    }
    Some(
        entries
            .into_iter()
            .filter(|e| !same_dir(e, dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\Kenny\AppData\Local\Programs\newsflash";

    #[test]
    fn path_is_appended_once_keeping_existing_entries_verbatim() {
        let current = r"%USERPROFILE%\.cargo\bin;C:\Tools;";
        assert_eq!(
            path_with(current, DIR).unwrap(),
            format!(r"%USERPROFILE%\.cargo\bin;C:\Tools;{DIR}")
        );
        assert_eq!(path_with("", DIR).unwrap(), DIR);
    }

    #[test]
    fn an_existing_entry_in_any_spelling_is_not_added_again() {
        let current = format!(r"C:\Tools;{}\", DIR.to_uppercase());
        assert_eq!(path_with(&current, DIR), None);
        assert_eq!(path_with(&format!("\"{DIR}\""), DIR), None);
    }

    #[test]
    fn removal_drops_only_our_entry() {
        let current = format!(r"C:\Tools;{DIR};%LOCALAPPDATA%\x");
        assert_eq!(
            path_without(&current, DIR).unwrap(),
            r"C:\Tools;%LOCALAPPDATA%\x"
        );
        assert_eq!(path_without(r"C:\Tools", DIR), None);
    }
}
