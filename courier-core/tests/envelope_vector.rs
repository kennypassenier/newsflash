//! AR4: the envelope v1 DRAFT is pinned by this vector. If pipeline-v2
//! changes the schema, this test is the tripwire — its failure means a
//! mini-round (SCOPE S7), not a quiet fixup.

use courier_core::envelope::parse_envelope;
use courier_core::toast::{Language, ToastSpec, Urgency, toast_spec};

const VECTOR: &[u8] = include_bytes!("vectors/envelope_v1.json");

#[test]
fn ar4_the_pinned_v1_vector_parses_with_every_modeled_field() {
    let env = parse_envelope(VECTOR).unwrap();
    assert_eq!(env.v, 1);
    assert_eq!(env.id, "01J6ZX3AC9V2N8KQ4T7R5E1WYD");
    assert_eq!(env.ts.as_deref(), Some("2026-08-28T21:41:07+02:00"));
    assert_eq!(env.source.as_deref(), Some("ha"));
    assert_eq!(env.kind.as_deref(), Some("notification"));
    assert_eq!(env.audience.as_deref(), Some("kenny"));
    assert_eq!(env.priority.as_deref(), Some("warning"));
    assert_eq!(env.title.as_ref().unwrap().nl.as_deref(), Some("Vriezer"));
    assert_eq!(env.title.as_ref().unwrap().en.as_deref(), Some("Freezer"));
}

#[test]
fn ar4_the_pinned_vector_maps_to_the_expected_toast() {
    let env = parse_envelope(VECTOR).unwrap();
    assert_eq!(
        toast_spec(&env, Language::Nl),
        ToastSpec {
            summary: "Vriezer".into(),
            body: "Temperatuur loopt op: -11 °C".into(),
            urgency: Urgency::Normal,
            expire_ms: 30_000,
            // AR11 amendment 2026-08-30: one icon per priority.
            icon: "dialog-warning",
            // The vector predates M10; no `actions` field → the default
            // pair (K12 amendment 2026-08-30).
            actions: vec![
                ("gelezen".into(), "Gelezen".into()),
                ("snooze".into(), "Snooze".into())
            ],
        }
    );
}

/// Two REAL messages read from the hub on 2026-09-24 (docs/PROPOSALS.md).
/// If pipeline-v2 changes how it sends buttons or ephemerality, these
/// fail first — the same tripwire role as the v1 draft vector above.
#[test]
fn live_plant_care_message_keeps_its_three_buttons() {
    let env = parse_envelope(include_bytes!("vectors/live_2026-09_action_buttons.json")).unwrap();
    let ids: Vec<String> = env
        .effective_actions()
        .unwrap()
        .into_iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids[0].starts_with("PLANTCARE_WATER_"));
    assert!(ids[2].starts_with("PLANTCARE_SKIP_"));
    let spec = toast_spec(&env, Language::Nl);
    assert_eq!(spec.actions[1].1, "Snooze 3 dagen");
    assert_eq!(
        courier_core::toast::resolve_link(
            env.click_url.as_deref().unwrap(),
            Some("http://10.10.10.2:8123")
        )
        .as_deref(),
        Some("http://10.10.10.2:8123/control-panel/plants")
    );
}

#[test]
fn live_ephemeral_message_lives_ten_minutes_by_default() {
    let env = parse_envelope(include_bytes!("vectors/live_2026-09_ephemeral.json")).unwrap();
    assert_eq!(env.ephemeral, Some(true));
    assert_eq!(
        courier_core::toast::lifetime_minutes(&env, &Default::default()),
        Some(10)
    );
    // No buttons of its own → the default pair.
    assert_eq!(toast_spec(&env, Language::Nl).actions[0].0, "gelezen");
}
