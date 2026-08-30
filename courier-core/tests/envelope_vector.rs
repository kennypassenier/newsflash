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
