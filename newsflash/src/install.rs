//! `newsflash install` / `uninstall` on Linux — the counterpart of
//! newsflash-win's installer (docs/WINDOWS.md), turning runbook R1's
//! manual steps into one command:
//!
//! 1. copy this binary to `~/.local/bin/newsflash` (XDG user bin);
//! 2. put `~/.local/bin` on PATH if it is not already, via removable
//!    drop-ins only: `environment.d` (the graphical session and the
//!    systemd user manager), a `fish/conf.d` file, and a marked block
//!    in `~/.bashrc` / `~/.zshrc` for the shells that exist;
//! 3. write the systemd user unit (the repo's `systemd/newsflash.service`
//!    with the binary path swapped; latch wrapper kept when latch is
//!    installed, AR10/M6), then `enable --now` (restart if running);
//! 4. drop a starter config if there is none.
//!
//! `uninstall` reverses 1–3 exactly — drop-ins deleted, marked blocks
//! cut out with every other line left untouched — and keeps config and
//! state, like the Windows side. Every PATH edit is recognisable by
//! `MARK`, so uninstall never needs a manifest.

use std::path::{Path, PathBuf};
use std::process::Command;

const MARK: &str = "newsflash install";
const BLOCK_START: &str = "# >>> newsflash install (PATH) >>>";
const BLOCK_END: &str = "# <<< newsflash install (PATH) <<<";
const UNIT_TEMPLATE: &str = include_str!("../../systemd/newsflash.service");
pub const CONFIG_EXAMPLE: &str = include_str!("../../config.example.toml");
const ICON: &[u8] = include_bytes!("../../newsflash-win/assets/app.png");

/// Where everything goes, derived from HOME/XDG — injectable for tests.
pub struct Layout {
    pub home: PathBuf,
    pub bin_dir: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Layout {
    pub fn from_env() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let data_dir = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        Layout {
            bin_dir: home.join(".local").join("bin"),
            home,
            config_dir,
            data_dir,
        }
    }

    pub fn installed_bin(&self) -> PathBuf {
        self.bin_dir.join("newsflash")
    }
    pub fn unit_path(&self) -> PathBuf {
        self.config_dir
            .join("systemd")
            .join("user")
            .join("newsflash.service")
    }
    fn environment_d(&self) -> PathBuf {
        self.config_dir
            .join("environment.d")
            .join("60-newsflash.conf")
    }
    fn fish_conf(&self) -> PathBuf {
        self.config_dir
            .join("fish")
            .join("conf.d")
            .join("newsflash.fish")
    }
    fn shell_rcs(&self) -> [PathBuf; 2] {
        [self.home.join(".bashrc"), self.home.join(".zshrc")]
    }
    pub fn user_config(&self) -> PathBuf {
        self.config_dir.join("newsflash").join("config.toml")
    }
    /// The app-launcher entry that reopens the setup wizard.
    fn desktop_entry(&self) -> PathBuf {
        self.data_dir
            .join("applications")
            .join("newsflash-setup.desktop")
    }
    fn icon(&self) -> PathBuf {
        self.data_dir
            .join("icons/hicolor/256x256/apps")
            .join("newsflash.png")
    }
}

/// `text` with our PATH block appended, or `None` if it is already there.
pub fn with_block(text: &str, bin_dir: &str) -> Option<String> {
    if text.contains(BLOCK_START) {
        return None;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "{BLOCK_START}\ncase \":$PATH:\" in *\":{bin_dir}:\"*) ;; *) export PATH=\"{bin_dir}:$PATH\" ;; esac\n{BLOCK_END}\n"
    ));
    Some(out)
}

/// `text` with our PATH block cut out, or `None` if it was not there.
/// Everything outside the markers survives byte for byte.
pub fn without_block(text: &str) -> Option<String> {
    let start = text.find(BLOCK_START)?;
    let end = text[start..].find(BLOCK_END)? + start + BLOCK_END.len();
    let end = if text[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    Some(format!("{}{}", &text[..start], &text[end..]))
}

/// The repo's unit, pointed at the installed binary. Keeps the latch
/// wrapper when latch exists; otherwise runs the binary directly (the
/// token then comes from `token_file`, the documented fallback).
pub fn render_unit(latch: Option<&Path>) -> String {
    let mut out = String::new();
    for line in UNIT_TEMPLATE.lines() {
        if line.starts_with("ExecStart=") {
            match latch {
                Some(l) => out.push_str(&format!(
                    "ExecStart={} run -- %h/.local/bin/newsflash",
                    l.display()
                )),
                None => out.push_str("ExecStart=%h/.local/bin/newsflash"),
            }
        } else if line.starts_with("WorkingDirectory=") && latch.is_none() {
            // Only latch needs the repo as its working directory.
            continue;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out.replace(
        "# newsflash systemd USER unit (K7, AR20). Install:",
        &format!("# newsflash systemd USER unit (K7, AR20) — written by `{MARK}`. Manual install:"),
    )
}

fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|e| e == dir))
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Our drop-in files carry `MARK`; never delete a file we did not write.
fn remove_if_ours(path: &Path) -> bool {
    let ours = std::fs::read_to_string(path).is_ok_and(|t| t.contains(MARK));
    ours && std::fs::remove_file(path).is_ok()
}

pub fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| format!("systemctl not available ({e})"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "systemctl --user {} failed ({status})",
            args.join(" ")
        ))
    }
}

/// Returns the PATH files written (empty = already on PATH).
fn add_to_path(layout: &Layout) -> Result<Vec<PathBuf>, String> {
    if on_path(&layout.bin_dir) {
        return Ok(Vec::new());
    }
    let bin = layout.bin_dir.display().to_string();
    let mut written = Vec::new();
    write(
        &layout.environment_d(),
        &format!("# added by {MARK} — removed by newsflash uninstall\nPATH={bin}:${{PATH}}\n"),
    )?;
    written.push(layout.environment_d());
    if layout.config_dir.join("fish").is_dir() || find_in_path("fish").is_some() {
        write(
            &layout.fish_conf(),
            &format!(
                "# added by {MARK} — removed by newsflash uninstall\nfish_add_path --global {bin}\n"
            ),
        )?;
        written.push(layout.fish_conf());
    }
    for rc in layout.shell_rcs() {
        let Ok(text) = std::fs::read_to_string(&rc) else {
            continue; // that shell is not in use here
        };
        if let Some(updated) = with_block(&text, &bin) {
            write(&rc, &updated)?;
            written.push(rc);
        }
    }
    Ok(written)
}

/// Returns the PATH files cleaned up.
fn remove_from_path(layout: &Layout) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for file in [layout.environment_d(), layout.fish_conf()] {
        if remove_if_ours(&file) {
            removed.push(file);
        }
    }
    for rc in layout.shell_rcs() {
        let Ok(text) = std::fs::read_to_string(&rc) else {
            continue;
        };
        if let Some(cleaned) = without_block(&text)
            && std::fs::write(&rc, cleaned).is_ok()
        {
            removed.push(rc);
        }
    }
    removed
}

/// What to install; the CLI uses the defaults, the wizard the user's
/// choices.
pub struct Options {
    pub add_to_path: bool,
    pub autostart: bool,
    /// Wrap the unit in `latch run` when latch is installed. The wizard
    /// turns this off when the user typed a token (it then lives in a
    /// 0600 token_file, and latch's env would otherwise override it).
    pub use_latch: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            add_to_path: true,
            autostart: true,
            use_latch: true,
        }
    }
}

/// The latch binary, if installed (PATH, then cargo's bin dir).
pub fn find_latch(layout: &Layout) -> Option<PathBuf> {
    find_in_path("latch").or_else(|| {
        let l = layout.home.join(".cargo").join("bin").join("latch");
        l.is_file().then_some(l)
    })
}

pub fn install(layout: &Layout) -> i32 {
    match install_with(layout, &Options::default(), &mut |line| println!("{line}")) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("install failed: {e}");
            1
        }
    }
}

pub fn install_with(
    layout: &Layout,
    options: &Options,
    log: &mut dyn FnMut(String),
) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("locating this binary: {e}"))?;
    let target = layout.installed_bin();
    let same = matches!(
        (std::fs::canonicalize(&current), std::fs::canonicalize(&target)),
        (Ok(a), Ok(b)) if a == b
    );
    if !same {
        std::fs::create_dir_all(&layout.bin_dir)
            .map_err(|e| format!("creating {}: {e}", layout.bin_dir.display()))?;
        // Copy beside, then rename: replacing a running binary in place
        // would fail ("text file busy"); a rename swaps it atomically.
        let tmp = layout.bin_dir.join(".newsflash.new");
        std::fs::copy(&current, &tmp).map_err(|e| format!("copying the binary: {e}"))?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("installing the binary: {e}"))?;
    }
    log(format!("installed to {}", target.display()));

    if options.add_to_path {
        let written = add_to_path(layout)?;
        if written.is_empty() {
            log(format!(
                "{} is already on your PATH",
                layout.bin_dir.display()
            ));
        } else {
            log(format!(
                "added {} to your PATH ({}) — open a NEW terminal to type just `newsflash`",
                layout.bin_dir.display(),
                written
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    } else if !remove_from_path(layout).is_empty() {
        log("removed the PATH entry (not wanted any more)".into());
    }

    write_menu_entry(layout, &target)?;
    log(format!(
        "added \"newsflash setup\" to the app menu ({})",
        layout.desktop_entry().display()
    ));

    let latch = if options.use_latch {
        find_latch(layout)
    } else {
        None
    };
    write(&layout.unit_path(), &render_unit(latch.as_deref()))?;
    log(format!(
        "wrote {} ({})",
        layout.unit_path().display(),
        if latch.is_some() {
            "token via latch"
        } else {
            "token via token_file"
        }
    ));

    if !layout.user_config().is_file() {
        write(&layout.user_config(), CONFIG_EXAMPLE)?;
        log(format!(
            "\nwrote a starter config to {}\nnext: set hub_url (and the token), then run \
             `newsflash install` again — or use `newsflash setup`.",
            layout.user_config().display()
        ));
        return Ok(());
    }

    match crate::chime::ensure(&layout.data_dir.join("newsflash"), &layout.user_config())? {
        crate::chime::Outcome::Configured(wav) => {
            log(format!("chime: sound_file now points at {}", wav.display()))
        }
        crate::chime::Outcome::KeptExisting(s) => {
            log(format!("chime: keeping the sound_file you set ({s})"))
        }
    }

    let enable = if options.autostart {
        "enable"
    } else {
        "disable"
    };
    let started = systemctl(&["daemon-reload"])
        .and_then(|_| systemctl(&[enable, "newsflash"]))
        .and_then(|_| systemctl(&["restart", "newsflash"]));
    match started {
        Ok(()) if options.autostart => {
            log("enabled and (re)started — it runs whenever you are logged in.".into())
        }
        Ok(()) => log("(re)started — not enabled at login, by your choice.".into()),
        Err(e) => {
            return Err(format!(
                "{e}. Start it yourself: systemctl --user daemon-reload && \
                 systemctl --user enable --now newsflash"
            ));
        }
    }
    Ok(())
}

fn write_menu_entry(layout: &Layout, bin: &Path) -> Result<(), String> {
    std::fs::create_dir_all(layout.icon().parent().unwrap_or(&layout.data_dir))
        .map_err(|e| format!("creating the icon dir: {e}"))?;
    std::fs::write(layout.icon(), ICON).map_err(|e| format!("writing the icon: {e}"))?;
    write(
        &layout.desktop_entry(),
        &format!(
            "[Desktop Entry]\n# written by {MARK} — removed by newsflash uninstall\n\
             Type=Application\nName=newsflash setup\n\
             Comment=Install, reconfigure or remove the kyu desktop notifications\n\
             Exec={} setup\nIcon=newsflash\nTerminal=false\nCategories=Settings;Utility;\n",
            bin.display()
        ),
    )
}

pub fn uninstall(layout: &Layout) -> i32 {
    match uninstall_with(layout, &mut |line| println!("{line}")) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("uninstall failed: {e}");
            1
        }
    }
}

pub fn uninstall_with(layout: &Layout, log: &mut dyn FnMut(String)) -> Result<(), String> {
    let _ = systemctl(&["disable", "--now", "newsflash"]);
    let unit_removed = std::fs::remove_file(layout.unit_path()).is_ok();
    let _ = systemctl(&["daemon-reload"]);
    if unit_removed {
        log("stopped, disabled and removed the systemd unit".into());
    }
    let removed = remove_from_path(layout);
    if !removed.is_empty() {
        log(format!(
            "removed the PATH entry from: {}",
            removed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if remove_if_ours(&layout.desktop_entry()) {
        let _ = std::fs::remove_file(layout.icon());
        log("removed \"newsflash setup\" from the app menu".into());
    }
    if std::fs::remove_file(layout.installed_bin()).is_ok() {
        log(format!("removed {}", layout.installed_bin().display()));
    }
    log(format!(
        "kept (delete by hand if wanted): {} and ~/.local/state/newsflash",
        layout.config_dir.join("newsflash").display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/home/k/.local/bin";

    #[test]
    fn the_path_block_is_added_once_and_removed_exactly() {
        let original = "alias ll='ls -l'\nexport EDITOR=vim";
        let with = with_block(original, BIN).unwrap();
        assert!(with.starts_with("alias ll='ls -l'\nexport EDITOR=vim\n"));
        assert!(with.contains(&format!("export PATH=\"{BIN}:$PATH\"")));
        assert_eq!(with_block(&with, BIN), None, "never twice");
        assert_eq!(
            without_block(&with).unwrap(),
            "alias ll='ls -l'\nexport EDITOR=vim\n"
        );
        assert_eq!(without_block(original), None);
    }

    #[test]
    fn content_after_the_block_survives_removal() {
        let with = with_block("a\n", BIN).unwrap() + "b\n";
        assert_eq!(without_block(&with).unwrap(), "a\nb\n");
    }

    #[test]
    fn the_unit_keeps_latch_when_present_and_drops_it_when_not() {
        let with_latch = render_unit(Some(Path::new("/home/k/.cargo/bin/latch")));
        assert!(
            with_latch
                .contains("ExecStart=/home/k/.cargo/bin/latch run -- %h/.local/bin/newsflash")
        );
        assert!(with_latch.contains("WorkingDirectory="));
        let plain = render_unit(None);
        assert!(plain.contains("\nExecStart=%h/.local/bin/newsflash\n"));
        assert!(!plain.contains("WorkingDirectory="));
        // Everything that makes the unit AR20-correct is untouched.
        for keep in [
            "PartOf=graphical-session.target",
            "TimeoutStopSec=45",
            "Restart=on-failure",
        ] {
            assert!(plain.contains(keep), "{keep}");
        }
    }
}
