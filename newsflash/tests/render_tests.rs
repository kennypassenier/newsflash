//! K2/K6/AR12/AR19/AR22 via PATH shims: argv, exit handling, the child
//! timeout, and the daemon probe. M10 adds the interactive-toast split:
//! `show_toast_interactive` must return promptly even while the
//! (shimmed) notify-send is still "showing", and `watch_interactive_toast`
//! must eventually report the click (or lack of one) and honor the
//! expire-based safety cap without capping a persistent (critical)
//! toast. What a shim cannot express — a real daemon actually
//! displaying buttons, a real click — is the live desktop drill
//! (AR18), recorded in the milestone report.
//!
//! Everything lives in ONE test: PATH and CLICK_FILE are process-global,
//! so parallel test threads mutating them across separate #[test] fns
//! would race (the file's original, still-true rule).

use courier_core::envelope::parse_envelope;
use courier_core::toast::{Language, toast_spec};
use newsflash::render::{
    RunOutcome, daemon_present, play_sound, run_with_timeout, show_toast_interactive,
    watch_interactive_toast,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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

/// The interactive notify-send shim: logs argv, sleeps `sleep_secs`
/// (simulating "showing"), then either echoes the content of
/// `$CLICK_FILE` (a simulated click) or prints nothing (timeout/dismiss),
/// and exits 0 — unless `fail_flag` exists, which exits 1 immediately.
fn install_interactive_shim(log: &std::path::Path, fail_flag: &std::path::Path, sleep_secs: f32) {
    let body = format!(
        "#!/bin/sh\necho \"$0 $*\" >> {log}\n[ -e {fail} ] && exit 1\nsleep {sleep_secs}\n\
         if [ -f \"$CLICK_FILE\" ]; then cat \"$CLICK_FILE\"; fi\nexit 0\n",
        log = log.display(),
        fail = fail_flag.display(),
    );
    install_shim("notify-send", &body);
}

/// All PATH-dependent cases in ONE test: PATH is process-global.
#[test]
fn l3_render_shell_via_path_shims() {
    let dir = shim_dir();
    let log = dir.join("calls.log");
    let fail_flag = dir.join("fail");
    let plain_body = format!(
        "#!/bin/sh\necho \"$0 $*\" >> {log}\n[ -e {flag} ] && exit 1\nexit 0\n",
        log = log.display(),
        flag = fail_flag.display()
    );
    install_shim("paplay", &plain_body);
    install_shim("busctl", &plain_body);
    install_interactive_shim(&log, &fail_flag, 0.0);
    let old_path = std::env::var("PATH").unwrap();
    // SAFETY (test-only): this is the only test in this binary, so no
    // concurrent env access.
    unsafe { std::env::set_var("PATH", format!("{}:{old_path}", dir.display())) };

    // K2 + M1 + AR12: exact argv per priority, `--` before positionals,
    // escaped body, default action buttons (K12).
    let env = parse_envelope(
        br#"{"v":1,"id":"x","priority":"critical","title":{"nl":"Vriezer"},"message":{"nl":"-11 < -18 & stijgt"}}"#,
    )
    .unwrap();
    let spec = toast_spec(&env, Language::Nl);
    let child = show_toast_interactive(&spec).expect("spawn ok");
    drop(child); // done with the grace-period check for this assertion
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(
        logged.contains(
            "--app-name=newsflash --urgency=critical --expire-time=0 --icon=dialog-error \
             -A gelezen=Gelezen -A snooze=Snooze -- Vriezer -11 &lt; -18 &amp; stijgt"
        ),
        "unexpected notify-send argv: {logged}"
    );

    // K12: a custom actions array replaces the default pair in argv.
    let env = parse_envelope(
        br#"{"v":1,"id":"x","title":{"nl":"a"},"actions":[
            {"id":"ik_pak_het_op","label":{"nl":"Ik pak het op"}}
        ]}"#,
    )
    .unwrap();
    let spec = toast_spec(&env, Language::Nl);
    drop(show_toast_interactive(&spec).expect("spawn ok"));
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(
        logged.contains("-A ik_pak_het_op=Ik pak het op --"),
        "custom action argv missing: {logged}"
    );

    // M1: the argv promise holds for every priority, not just critical
    // (hardening gap G12).
    let env = parse_envelope(br#"{"v":1,"id":"x","priority":"info","title":{"nl":"I"}}"#).unwrap();
    let info_spec = toast_spec(&env, Language::Nl);
    drop(show_toast_interactive(&info_spec).unwrap());
    let env =
        parse_envelope(br#"{"v":1,"id":"x","priority":"warning","title":{"nl":"W"}}"#).unwrap();
    drop(show_toast_interactive(&toast_spec(&env, Language::Nl)).unwrap());
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("--urgency=normal --expire-time=10000 --icon=dialog-information"));
    assert!(logged.contains("--urgency=normal --expire-time=30000 --icon=dialog-warning"));

    // K3: a failing spawn/instant-exit reports Err (the loop nacks).
    std::fs::write(&fail_flag, "").unwrap();
    let env = parse_envelope(br#"{"v":1,"id":"x","title":{"nl":"F"}}"#).unwrap();
    assert!(show_toast_interactive(&toast_spec(&env, Language::Nl)).is_err());
    std::fs::remove_file(&fail_flag).unwrap();

    // AR22: the probe follows the daemon.
    std::fs::write(&fail_flag, "").unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if logged.contains("paplay") {
            assert!(logged.contains("-- "), "paplay must get the -- separator");
            break;
        }
        assert!(Instant::now() < deadline, "paplay shim never ran");
        std::thread::sleep(Duration::from_millis(50));
    }

    // AR19: a hanging child is killed at the deadline, not waited out.
    install_shim("hang", "#!/bin/sh\nsleep 30\n");
    let started = Instant::now();
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

    // --- M10: the interactive lifecycle, same PATH/dir, still serial ---

    let plain_env = parse_envelope(br#"{"v":1,"id":"x","title":{"nl":"a"}}"#).unwrap();
    let plain_spec = toast_spec(&plain_env, Language::Nl);
    let critical_env =
        parse_envelope(br#"{"v":1,"id":"x","priority":"critical","title":{"nl":"a"}}"#).unwrap();
    let critical_spec = toast_spec(&critical_env, Language::Nl);

    // Phase A: a real click, and proof the spawn call itself doesn't
    // block for it. Shim sleeps 1s — well past SPAWN_GRACE (300ms).
    let click_file = dir.join("click");
    std::fs::write(&click_file, "snooze").unwrap();
    install_interactive_shim(&log, &fail_flag, 1.0);
    unsafe { std::env::set_var("CLICK_FILE", &click_file) };
    let started = Instant::now();
    let child = show_toast_interactive(&info_spec).expect("spawn ok");
    assert!(
        started.elapsed() < Duration::from_millis(600),
        "show_toast_interactive blocked for {:?} — it must detach, not wait for the click",
        started.elapsed()
    );
    let (tx, rx) = mpsc::channel();
    watch_interactive_toast(child, Some(Duration::from_secs(5)), move |action| {
        tx.send(action).unwrap();
    });
    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("watcher never reported a result");
    assert_eq!(result, Some("snooze".to_string()));
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "the watcher must have waited out the full interactive session"
    );

    // Phase B: no click, no content on stdout → None.
    unsafe { std::env::remove_var("CLICK_FILE") };
    install_interactive_shim(&log, &fail_flag, 0.2);
    let child = show_toast_interactive(&plain_spec).expect("spawn ok");
    let (tx, rx) = mpsc::channel();
    watch_interactive_toast(child, Some(Duration::from_secs(3)), move |action| {
        tx.send(action).unwrap();
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), None);

    // Phase C: the safety cap kills a toast that outlives it (bounded
    // case — info/warning). Shim sleeps far longer than the cap.
    install_interactive_shim(&log, &fail_flag, 5.0);
    let child = show_toast_interactive(&plain_spec).expect("spawn ok");
    let cap_started = Instant::now();
    let (tx, rx) = mpsc::channel();
    watch_interactive_toast(child, Some(Duration::from_millis(300)), move |action| {
        tx.send(action).unwrap();
    });
    let result = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the safety cap must fire well before the 5s shim would finish on its own");
    assert_eq!(result, None);
    assert!(
        cap_started.elapsed() < Duration::from_secs(2),
        "the cap did not bound the wait: {:?}",
        cap_started.elapsed()
    );

    // Phase D: no cap (critical) must NOT kill early — the actual bug
    // a live drill found (Kenny, 2026-08-30): once the process dies,
    // Plasma drops the action buttons even though the notification
    // body stays visible, so an artificial cap here would silently
    // break "critical lasts until dismissed" long before anyone answers.
    install_interactive_shim(&log, &fail_flag, 0.8);
    let child = show_toast_interactive(&critical_spec).expect("spawn ok");
    let (tx, rx) = mpsc::channel();
    watch_interactive_toast(child, None, move |action| {
        tx.send(action).unwrap();
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "an uncapped watcher must not report early — phase C proved 300ms is enough for a \
         capped watcher of the same shim family to already have fired"
    );
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), None);

    unsafe { std::env::set_var("PATH", old_path) };
}
