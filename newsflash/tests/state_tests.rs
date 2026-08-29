//! K4/AR6 at the filesystem level (hardening gap G4): the store
//! round-trips through a real file, a corrupt file starts empty, and
//! save creates its directory.

use courier_core::dedup::SeenSet;
use newsflash::state;
use std::path::PathBuf;

fn fresh_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-state-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("nested").join("seen.json")
}

#[test]
fn k4_the_store_round_trips_through_a_real_file() {
    let path = fresh_path("roundtrip");
    let mut seen = SeenSet::new(8);
    seen.insert("hub-1");
    seen.insert("hub-2");
    state::save(&path, &seen); // must create the nested dir itself
    let restored = state::load(&path);
    assert!(restored.contains("hub-1") && restored.contains("hub-2"));
}

#[test]
fn ar6_a_missing_file_is_a_silent_empty_start() {
    let restored = state::load(&fresh_path("missing"));
    assert!(!restored.contains("anything"));
}

#[test]
fn ar6_a_corrupt_file_starts_empty_instead_of_crashing() {
    let path = fresh_path("corrupt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{definitely not a json array").unwrap();
    let restored = state::load(&path);
    assert!(!restored.contains("anything"));
}

#[test]
fn ar6_save_replaces_atomically_leaving_no_temp_file() {
    let path = fresh_path("atomic");
    let mut seen = SeenSet::new(8);
    seen.insert("a");
    state::save(&path, &seen);
    seen.insert("b");
    state::save(&path, &seen);
    assert!(path.exists());
    assert!(!path.parent().unwrap().join("seen.json.tmp").exists());
    let restored = state::load(&path);
    assert!(restored.contains("a") && restored.contains("b"));
}
