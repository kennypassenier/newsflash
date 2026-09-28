//! `newsflash demo`: a local tour of what a Windows toast can do, built
//! through the exact same pipeline as a hub message (envelope JSON →
//! `wintoast::build_toast` → `toast::show_built`), so what you see is
//! what a producer would get. Nothing touches the hub; clicks on demo
//! toasts carry a demo flag and are only ever printed/logged.

use crate::activator;
use crate::app::{daemon_running, ensure_assets, handle_click, is_set, stop_requested_flag};
use crate::toast::{existing, notifier, show_built};
use courier_core::envelope::parse_envelope;
use courier_core::toast::Language;
use courier_core::wintoast::{BuildInput, CriticalScenario, build_toast, logo_asset};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Step {
    what: &'static str,
    envelope: &'static str,
    hero: bool,
    pause_ms: u64,
}

const STEPS: &[Step] = &[
    Step {
        what: "info: short popup, source as header + attribution, default Gelezen/Snooze",
        envelope: r#"{"v":1,"id":"demo-1","source":"home-assistant","priority":"info",
            "title":{"nl":"Wasmachine klaar","en":"Washing machine done"},
            "message":{"nl":"Programma Katoen 40° is afgelopen.","en":"Cotton 40° has finished."}}"#,
        hero: false,
        pause_ms: 1500,
    },
    Step {
        what: "warning: long popup, hero image, a link button + body click opens a URL",
        envelope: r#"{"v":1,"id":"demo-2","source":"frigate","priority":"warning",
            "title":{"nl":"Beweging aan de voordeur","en":"Motion at the front door"},
            "message":{"nl":"Persoon gedetecteerd om 14:02.","en":"Person detected at 14:02."},
            "click_url":"http://homeassistant.local:8123/",
            "actions":[{"id":"open_camera","label":{"nl":"Open camera","en":"Open camera"},
                        "url":"http://homeassistant.local:8123/lovelace/cameras"},
                       {"id":"gelezen","label":{"nl":"Gezien","en":"Seen"}}]}"#,
        hero: true,
        pause_ms: 1500,
    },
    Step {
        what: "inputs: quick-reply text box + dropdown, coloured buttons (green/red)",
        envelope: r#"{"v":1,"id":"demo-3","source":"kyu","priority":"info",
            "title":{"nl":"Iemand belt aan","en":"Someone is at the door"},
            "message":{"nl":"Stuur een bericht naar de deurbel of snooze.","en":"Reply via the doorbell speaker or snooze."},
            "inputs":[{"id":"reply","placeholder":{"nl":"Bericht voor de deurbel…","en":"Message for the doorbell…"}},
                      {"id":"snooze_minutes","type":"selection","default":"60",
                       "title":{"nl":"Snooze voor","en":"Snooze for"},
                       "choices":[{"id":"5","label":{"nl":"5 minuten","en":"5 minutes"}},
                                  {"id":"60","label":{"nl":"1 uur","en":"1 hour"}},
                                  {"id":"1440","label":{"nl":"24 uur","en":"24 hours"}}]}],
            "actions":[{"id":"send","label":{"nl":"Verstuur","en":"Send"},"style":"success"},
                       {"id":"snooze","label":{"nl":"Snooze","en":"Snooze"}},
                       {"id":"ignore","label":{"nl":"Negeer","en":"Ignore"},"style":"critical"}]}"#,
        hero: false,
        pause_ms: 1500,
    },
    Step {
        what: "live progress: same tag + progress → updated in place, no new popups",
        envelope: r#"{"v":1,"id":"demo-4a","source":"home-assistant","tag":"demo-dishwasher",
            "title":{"nl":"Vaatwasser","en":"Dishwasher"},
            "progress":{"value":0.1,"status":{"nl":"Voorspoelen","en":"Pre-rinse"},"label":"1/4"},
            "actions":[{"id":"gelezen","label":{"nl":"Verbergen","en":"Hide"}}]}"#,
        hero: false,
        pause_ms: 2000,
    },
    Step {
        what: "  … update 2/4",
        envelope: r#"{"v":1,"id":"demo-4b","source":"home-assistant","tag":"demo-dishwasher",
            "title":{"nl":"Vaatwasser","en":"Dishwasher"},
            "progress":{"value":0.45,"status":{"nl":"Wassen","en":"Washing"},"label":"2/4"}}"#,
        hero: false,
        pause_ms: 2000,
    },
    Step {
        what: "  … update 3/4",
        envelope: r#"{"v":1,"id":"demo-4c","source":"home-assistant","tag":"demo-dishwasher",
            "title":{"nl":"Vaatwasser","en":"Dishwasher"},
            "progress":{"value":0.8,"status":{"nl":"Drogen","en":"Drying"},"label":"3/4"}}"#,
        hero: false,
        pause_ms: 2000,
    },
    Step {
        what: "  … update 4/4",
        envelope: r#"{"v":1,"id":"demo-4d","source":"home-assistant","tag":"demo-dishwasher",
            "title":{"nl":"Vaatwasser klaar","en":"Dishwasher done"},
            "progress":{"value":1.0,"status":{"nl":"Klaar","en":"Done"},"label":"4/4"}}"#,
        hero: false,
        pause_ms: 1500,
    },
    Step {
        what: "live pipeline-v2 buttons (data.action_buttons) + a relative link resolved against link_base_url",
        envelope: r#"{"v":1,"id":"demo-6","source":"ha","priority":"warning","click_url":"/control-panel/plants",
            "title":{"nl":"2 planten hebben water nodig","en":"2 plants need water"},
            "message":{"nl":"Voel eerst of de aarde droog is.","en":"Feel the soil first."},
            "data":{"action_buttons":[{"action":"PLANTCARE_WATER_demo","title":"Water gegeven"},
                                      {"action":"PLANTCARE_SNOOZE_demo","title":"Snooze 3 dagen"},
                                      {"action":"PLANTCARE_SKIP_demo","title":"Nog te nat"}]}}"#,
        hero: false,
        pause_ms: 1500,
    },
    Step {
        what: "ephemeral: removes itself from Notification Center after 2 minutes",
        envelope: r#"{"v":1,"id":"demo-7","source":"ha","ephemeral":true,"expires_in_minutes":2,
            "title":{"nl":"Beweging","en":"Movement"},
            "message":{"nl":"Vijf minuten buiten. Deur open en gaan.","en":"Five minutes outside. Door open, go."}}"#,
        hero: false,
        pause_ms: 1500,
    },
    Step {
        what: "critical: stays on screen until answered (scenario reminder)",
        envelope: r#"{"v":1,"id":"demo-5","source":"home-assistant","priority":"critical",
            "title":{"nl":"Rookmelder keuken","en":"Kitchen smoke alarm"},
            "message":{"nl":"Rook gedetecteerd. Dit blijft staan tot je antwoordt.","en":"Smoke detected. This stays until you answer."}}"#,
        hero: false,
        pause_ms: 0,
    },
];

pub fn run() -> i32 {
    let notifier = match notifier() {
        Ok(n) if crate::registry::is_registered() => n,
        _ => {
            eprintln!(
                "newsflash is not registered with Windows yet — run `newsflash install` first."
            );
            return 1;
        }
    };
    let assets = ensure_assets();
    let hero = existing(&assets.join("demo-hero.png"));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Take the clicks ourselves unless the daemon already does.
    let (tx, rx) = mpsc::channel();
    let registered = if daemon_running() {
        println!("(the courier is running, so clicks go to its log — see `newsflash status`)\n");
        None
    } else {
        activator::register(tx).ok()
    };

    let mut live = VecDeque::new();
    for (i, step) in STEPS.iter().enumerate() {
        println!("{}", step.what);
        let env = parse_envelope(step.envelope.as_bytes()).expect("demo envelopes are valid");
        let logo = existing(&assets.join(logo_asset(env.priority.as_deref())));
        let built = build_toast(
            &env,
            &BuildInput {
                hub_id: &format!("demo-{}", i + 1),
                published_at_ms: now,
                language: Language::Nl,
                critical_scenario: CriticalScenario::Reminder,
                logo_uri: logo.as_deref(),
                hero_uri: if step.hero { hero.as_deref() } else { None },
                silent: false,
                demo: true,
                lifetimes: courier_core::toast::Lifetimes::default(),
                popup: courier_core::toast::PopupDurations::default(),
                hold_popup: false,
                fallback_tag: None,
                link_base: Some("http://10.10.10.2:8123"),
            },
        );
        if let Err(e) = show_built(&notifier, &built, &format!("demo-{}", i + 1), &mut live) {
            eprintln!("  failed: {e}");
        }
        std::thread::sleep(Duration::from_millis(step.pause_ms));
    }

    if registered.is_some() {
        println!(
            "\nClick buttons / type in the toasts — clicks print here. Ctrl+C to stop (2 min max)."
        );
        let stop = stop_requested_flag();
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        while !is_set(&stop) && std::time::Instant::now() < deadline {
            if let Ok(click) = rx.recv_timeout(Duration::from_millis(250)) {
                handle_click(&click, None, true);
            }
        }
    }
    0
}
