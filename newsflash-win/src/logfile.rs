//! The journal's stand-in (M11 on Windows): a size-capped log file with
//! UTC timestamps, installed as the `newsflash::logx` sink. The
//! windowless daemon has no console, so this file is where the startup
//! summary, every lifecycle line and every remedy end up.

use courier_core::wintoast::rfc3339_utc;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Past this the file rotates to `<name>.1` (one generation kept).
pub const MAX_BYTES: u64 = 1024 * 1024;

pub struct LogFile {
    path: PathBuf,
    file: Mutex<Option<File>>,
    /// Also print to stderr (the console binary, run in the foreground).
    echo: bool,
}

impl LogFile {
    pub fn open(path: &Path, echo: bool) -> Self {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        LogFile {
            path: path.to_path_buf(),
            file: Mutex::new(open_append(path)),
            echo,
        }
    }

    pub fn write(&self, priority: u8, msg: &str) {
        let level = match priority {
            0..=3 => "ERROR",
            4 => "WARN ",
            _ => "INFO ",
        };
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("{} {level} {msg}\n", rfc3339_utc(secs));
        if self.echo {
            eprint!("{line}");
        }
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        if guard
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() > MAX_BYTES)
        {
            *guard = None; // close before renaming (Windows holds the lock)
            let _ = std::fs::rename(&self.path, rotated(&self.path));
            *guard = open_append(&self.path);
        }
        if let Some(f) = guard.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn open_append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

pub fn rotated(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

/// The last `n` lines of the log, for `newsflash status`.
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nf-win-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("newsflash.log")
    }

    #[test]
    fn m11_lines_carry_a_timestamp_and_level() {
        let path = temp("lines");
        let log = LogFile::open(&path, false);
        log.write(6, "starting");
        log.write(4, "careful");
        log.write(3, "broken");
        let lines = tail(&path, 10);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("INFO  starting"), "{}", lines[0]);
        assert!(lines[1].contains("WARN  careful"));
        assert!(lines[2].contains("ERROR broken"));
        assert!(lines[0].starts_with("20") && lines[0].contains('T'));
    }

    #[test]
    fn the_file_rotates_past_the_cap_keeping_one_generation() {
        let path = temp("rotate");
        let log = LogFile::open(&path, false);
        let big = "x".repeat(64 * 1024);
        for _ in 0..20 {
            log.write(6, &big);
        }
        assert!(rotated(&path).exists());
        assert!(std::fs::metadata(&path).unwrap().len() <= MAX_BYTES + 70 * 1024);
    }

    #[test]
    fn tail_of_a_missing_file_is_empty() {
        assert!(tail(Path::new("/definitely/not/here.log"), 5).is_empty());
    }
}
