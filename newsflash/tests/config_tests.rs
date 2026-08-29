//! M2/AR10: every broken-config class fails with a message naming the
//! field and the remedy; token resolution follows the AR10 order.

use newsflash::config;
use std::path::PathBuf;

fn write_config(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("newsflash-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// All env-sensitive cases in ONE test: tests share the process
/// environment, so KYU_TOKEN mutations must not run in parallel.
#[test]
fn m2_ar10_config_validation_names_field_and_remedy() {
    // SAFETY (test-only): single-threaded within this test; no other
    // test in this binary touches KYU_TOKEN.
    unsafe { std::env::remove_var("KYU_TOKEN") };

    // Missing file → remedy mentions the example.
    let err = config::load(&PathBuf::from("/nonexistent/nope.toml")).unwrap_err();
    assert!(err.contains("config.example.toml"), "{err}");

    // Invalid TOML.
    let p = write_config("bad.toml", "hub_url = [broken");
    assert!(config::load(&p).unwrap_err().contains("not valid TOML"));

    // Missing hub_url.
    let p = write_config("nohub.toml", "topic = \"notify.kenny\"");
    assert!(config::load(&p).unwrap_err().contains("hub_url"));

    // https refused with the N3 explanation.
    let p = write_config("https.toml", "hub_url = \"https://10.0.0.1:8080\"");
    assert!(config::load(&p).unwrap_err().contains("http://"));

    // Inline token refused (AR10).
    let p = write_config("inline.toml", "hub_url = \"http://h:1\"\ntoken = \"oops\"");
    let err = config::load(&p).unwrap_err();
    assert!(err.contains("inline token"), "{err}");

    // ttl_minutes = 0 refused.
    let p = write_config("ttl0.toml", "hub_url = \"http://h:1\"\nttl_minutes = 0");
    assert!(config::load(&p).unwrap_err().contains("ttl_minutes"));

    // G16: an explicitly empty topic/subscription is refused, not
    // silently turned into a malformed URL.
    let p = write_config("emptytopic.toml", "hub_url = \"http://h:1\"\ntopic = \"\"");
    assert!(config::load(&p).unwrap_err().contains("topic"));
    let p = write_config(
        "emptysub.toml",
        "hub_url = \"http://h:1\"\nsubscription = \" \"",
    );
    assert!(config::load(&p).unwrap_err().contains("subscription"));

    // G16: an absurd ttl cannot overflow downstream arithmetic.
    let p = write_config(
        "ttlhuge.toml",
        "hub_url = \"http://h:1\"\nttl_minutes = 99999999999",
    );
    assert!(config::load(&p).unwrap_err().contains("year"));

    // Unknown language refused.
    let p = write_config("lang.toml", "hub_url = \"http://h:1\"\nlanguage = \"de\"");
    assert!(config::load(&p).unwrap_err().contains("nl"));

    // Missing sound file refused with remedy.
    let p = write_config(
        "snd.toml",
        "hub_url = \"http://h:1\"\nsound_file = \"/nonexistent.wav\"",
    );
    assert!(config::load(&p).unwrap_err().contains("sound_file"));

    // No token anywhere → remedy names latch and the /apps page.
    let p = write_config("ok.toml", "hub_url = \"http://h:1\"");
    let err = config::load(&p).unwrap_err();
    assert!(err.contains("latch") && err.contains("/apps"), "{err}");

    // Empty env token is its own error (critic on AR10).
    unsafe { std::env::set_var("KYU_TOKEN", "  ") };
    let err = config::load(&p).unwrap_err();
    assert!(err.contains("empty"), "{err}");

    // A real env token wins and the defaults land.
    unsafe { std::env::set_var("KYU_TOKEN", "tok-123") };
    let cfg = config::load(&p).unwrap();
    assert_eq!(cfg.token, "tok-123");
    assert_eq!(cfg.topic, "notify.kenny");
    assert_eq!(cfg.subscription, "desktop");
    assert_eq!(cfg.ttl_ms, 600_000);
    unsafe { std::env::remove_var("KYU_TOKEN") };

    // token_file: group/other-readable refused with the chmod remedy.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tokf = write_config("token.txt", "file-tok\n");
        std::fs::set_permissions(&tokf, std::fs::Permissions::from_mode(0o644)).unwrap();
        let p = write_config(
            "tokfile.toml",
            &format!(
                "hub_url = \"http://h:1\"\ntoken_file = \"{}\"",
                tokf.display()
            ),
        );
        let err = config::load(&p).unwrap_err();
        assert!(err.contains("chmod 600"), "{err}");

        std::fs::set_permissions(&tokf, std::fs::Permissions::from_mode(0o600)).unwrap();
        let cfg = config::load(&p).unwrap();
        assert_eq!(cfg.token, "file-tok");
    }
}
