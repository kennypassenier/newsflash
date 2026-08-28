//! Dedup store file I/O (K4, AR6): atomic writes, fail-open reads.

use courier_core::dedup::{DEFAULT_CAPACITY, SeenSet};
use std::path::PathBuf;

pub fn state_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("state")
        });
    base.join("desk-courier").join("seen.json")
}

/// Missing file = first run (silent); corrupt file = logged, empty
/// start — fail-open, a duplicate toast beats a lost one (AR6).
pub fn load(path: &PathBuf) -> SeenSet {
    match std::fs::read_to_string(path) {
        Err(_) => SeenSet::new(DEFAULT_CAPACITY),
        Ok(text) => SeenSet::from_json(&text, DEFAULT_CAPACITY).unwrap_or_else(|| {
            crate::logx::warn(&format!(
                "dedup store {} is corrupt — starting empty (worst case: one duplicate toast)",
                path.display()
            ));
            SeenSet::new(DEFAULT_CAPACITY)
        }),
    }
}

/// Temp + rename in the same directory (standing rule 12).
pub fn save(path: &PathBuf, seen: &SeenSet) {
    let Some(dir) = path.parent() else { return };
    if let Err(e) = std::fs::create_dir_all(dir) {
        crate::logx::warn(&format!("cannot create state dir {}: {e}", dir.display()));
        return;
    }
    let tmp = dir.join("seen.json.tmp");
    if let Err(e) = std::fs::write(&tmp, seen.to_json()) {
        crate::logx::warn(&format!("cannot write dedup store: {e}"));
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        crate::logx::warn(&format!("cannot move dedup store into place: {e}"));
    }
}
