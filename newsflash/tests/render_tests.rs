//! K2/K6/AR12/AR19/AR22 via PATH shims: argv, exit handling, the child
//! timeout, and the daemon probe. What a shim cannot express — a real
//! daemon actually displaying the toast — is the live desktop drill
//! (AR18), recorded in the milestone report.

use courier_core::envelope::parse_envelope;
use courier_core::toast::{Language, toast_spec};
use newsflash::render::{RunOutcome, daemon_present, play_sound, run_with_timeout, show_toast};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn shim_dir() -> PathBuf {
    std::env::temp_dir().join(format!("dc-shims-{}", std::process::id()))
}

fn install_shim(name: &str, script: &str) {
    let dir = shim_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// All PATH-dependent cases in ONE test: PATH is process-global.
#[test]
fn l3_render_shell_via_path_shims() {
    let dir = shim_dir();
    let log = dir.join("calls.log");
    let fail_flag = dir.join("fail");
    // Shims log their argv one line each; the fail flag flips exit 1.
    let shim_body = format!(
        "#!/bin/sh\necho \"$0 $*\" >> {log}\n[ -e {flag} ] && exit 1\nexit 0\n",
        log = log.display(),
        flag = fail_flag.display()
    );
    install_shim("notify-send", &shim_body);
    install_shim("paplay", &shim_body);
    install_shim("busctl", &shim_body);
    let old_path = std::env::var("PATH").unwrap();
    // SAFETY (test-only): this is the only test in this binary, so no
    // concurrent env access.
    unsafe { std::env::set_var("PATH", format!("{}:{old_path}", dir.display())) };

    // K2 + M1 + AR12: exact argv per priority, `--` before positionals,
    // escaped body.
    let env = parse_envelope(
        br#"{"v":1,"id":"x","priority":"critical","title":{"nl":"Vriezer"},"message":{"nl":"-11 < -18 & stijgt"}}"#,
    )
    .unwrap();
    let spec = toast_spec(&env, Language::Nl);
    assert_eq!(show_toast(&spec), RunOutcome::Ok);
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(
        logged.contains(
            "--app-name=newsflash --urgency=critical --expire-time=0 -- Vriezer -11 &lt; -18 &amp; stijgt"
        ),
        "unexpected notify-send argv: {logged}"
    );

    // M1: the argv promise holds for every priority, not just critical
    // (hardening gap G12).
    let env = parse_envelope(br#"{"v":1,"id":"x","priority":"info","title":{"nl":"I"}}"#).unwrap();
    assert_eq!(show_toast(&toast_spec(&env, Language::Nl)), RunOutcome::Ok);
    let env =
        parse_envelope(br#"{"v":1,"id":"x","priority":"warning","title":{"nl":"W"}}"#).unwrap();
    assert_eq!(show_toast(&toast_spec(&env, Language::Nl)), RunOutcome::Ok);
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("--urgency=normal --expire-time=10000 -- I"));
    assert!(logged.contains("--urgency=normal --expire-time=30000 -- W"));

    // K3: a failing renderer reports Failed (the loop nacks).
    std::fs::write(&fail_flag, "").unwrap();
    assert!(matches!(show_toast(&spec), RunOutcome::Failed(_)));

    // AR22: the probe follows the daemon.
    assert!(!daemon_present(), "probe must fail while busctl fails");
    std::fs::remove_file(&fail_flag).unwrap();
    assert!(daemon_present());
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(
        logged.contains("GetServerInformation"),
        "probe must query, never notify: {logged}"
    );

    // K6: the chime fires as a detached spawn and never blocks.
    std::fs::write(&log, "").unwrap();
    play_sound(&dir.join("chime.ogg"));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if logged.contains("paplay") {
            assert!(logged.contains("-- "), "paplay must get the -- separator");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "paplay shim never ran"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // AR19: a hanging child is killed at the deadline, not waited out.
    install_shim("hang", "#!/bin/sh\nsleep 30\n");
    let started = std::time::Instant::now();
    let outcome = run_with_timeout(Command::new("hang"), Duration::from_secs(1));
    match outcome {
        RunOutcome::Failed(reason) => assert!(reason.contains("timed out"), "{reason}"),
        other => panic!("expected timeout, got {other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(5));

    // Spawn failure is Failed, not a panic.
    assert!(matches!(
        run_with_timeout(
            Command::new(dir.join("does-not-exist")),
            Duration::from_secs(1)
        ),
        RunOutcome::Failed(_)
    ));

    unsafe { std::env::set_var("PATH", old_path) };
}
