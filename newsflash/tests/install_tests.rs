//! `newsflash install` / `uninstall` on Linux, run as the REAL binary
//! against a throwaway HOME with a PATH-shimmed `systemctl` — the live
//! user manager is never touched (a stray enable would add a second
//! consumer to the `desktop` subscription).

use std::path::{Path, PathBuf};
use std::process::Command;

struct Home {
    root: PathBuf,
    shims: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("nf-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shims = root.join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let log = root.join("systemctl.log");
        let shim = shims.join("systemctl");
        std::fs::write(
            &shim,
            format!("#!/bin/sh\necho \"$@\" >> {}\nexit 0\n", log.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Home { root, shims }
    }

    fn run(&self, cmd: &str) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_newsflash"))
            .arg(cmd)
            .env("HOME", &self.root)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            // No ~/.local/bin, no latch, no fish: a deterministic PATH.
            .env("PATH", format!("{}:/usr/bin:/bin", self.shims.display()))
            .output()
            .unwrap();
        assert!(out.status.success(), "{cmd} failed: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn systemctl_calls(&self) -> String {
        std::fs::read_to_string(self.p("systemctl.log")).unwrap_or_default()
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn install_adds_path_and_unit_and_uninstall_takes_exactly_that_away() {
    let home = Home::new("roundtrip");
    let bashrc_before = "alias ll='ls -l'\n";
    std::fs::write(home.p(".bashrc"), bashrc_before).unwrap();
    std::fs::create_dir_all(home.p(".config/fish")).unwrap();

    // 1st install: binary, PATH, unit, starter config — no start yet.
    let out = home.run("install");
    assert!(home.p(".local/bin/newsflash").is_file());
    assert!(out.contains("added"), "{out}");
    assert!(
        read(&home.p(".config/environment.d/60-newsflash.conf")).contains(&format!(
            "PATH={}:${{PATH}}",
            home.p(".local/bin").display()
        ))
    );
    assert!(read(&home.p(".config/fish/conf.d/newsflash.fish")).contains("fish_add_path"));
    let bashrc = read(&home.p(".bashrc"));
    assert!(bashrc.starts_with(bashrc_before) && bashrc.contains("export PATH="));
    assert!(
        !home.p(".zshrc").exists(),
        "no zsh in use → no .zshrc created"
    );
    let unit = read(&home.p(".config/systemd/user/newsflash.service"));
    assert!(
        unit.contains("\nExecStart=%h/.local/bin/newsflash\n"),
        "{unit}"
    );
    assert!(home.p(".config/newsflash/config.toml").is_file());
    let entry = read(&home.p(".local/share/applications/newsflash-setup.desktop"));
    assert!(entry.contains(&format!(
        "Exec={} setup",
        home.p(".local/bin/newsflash").display()
    )));
    assert!(
        home.p(".local/share/icons/hicolor/256x256/apps/newsflash.png")
            .is_file()
    );
    assert_eq!(
        home.systemctl_calls(),
        "",
        "no start before a config exists"
    );

    // 2nd install: now enables + restarts; PATH edits never doubled.
    home.run("install");
    assert_eq!(read(&home.p(".bashrc")).matches("export PATH=").count(), 1);
    assert!(
        home.systemctl_calls()
            .contains("--user daemon-reload\n--user enable newsflash\n--user restart newsflash\n"),
        "{}",
        home.systemctl_calls()
    );

    // Uninstall: everything install added is gone, the rest untouched.
    home.run("uninstall");
    assert!(
        home.systemctl_calls()
            .contains("--user disable --now newsflash")
    );
    assert!(!home.p(".config/systemd/user/newsflash.service").exists());
    assert!(!home.p(".config/environment.d/60-newsflash.conf").exists());
    assert!(!home.p(".config/fish/conf.d/newsflash.fish").exists());
    assert_eq!(read(&home.p(".bashrc")), bashrc_before);
    assert!(!home.p(".local/bin/newsflash").exists());
    assert!(
        !home
            .p(".local/share/applications/newsflash-setup.desktop")
            .exists()
    );
    assert!(
        !home
            .p(".local/share/icons/hicolor/256x256/apps/newsflash.png")
            .exists()
    );
    assert!(
        home.p(".config/newsflash/config.toml").is_file(),
        "config is kept"
    );
}

#[test]
fn a_bin_dir_already_on_path_is_left_alone_and_foreign_files_are_never_deleted() {
    let home = Home::new("onpath");
    std::fs::create_dir_all(home.p(".config/environment.d")).unwrap();
    let foreign = home.p(".config/environment.d/60-newsflash.conf");
    std::fs::write(&foreign, "PATH=/somebody/elses:${PATH}\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_newsflash"))
        .arg("install")
        .env("HOME", &home.root)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                home.shims.display(),
                home.p(".local/bin").display()
            ),
        )
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("already on your PATH"), "{stdout}");
    assert_eq!(read(&foreign), "PATH=/somebody/elses:${PATH}\n");

    home.run("uninstall");
    assert!(
        foreign.exists(),
        "a file without our mark is not ours to delete"
    );
}
