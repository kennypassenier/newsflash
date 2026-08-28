//! The settle table (AR5): every received message ends in exactly one
//! settlement, keyed by the HUB message id (never the payload id — a
//! poison payload has no parseable id, redelivery reuses the hub id,
//! and an intentional republish must still toast). Split at the render
//! boundary so both halves stay pure — the shell renders in between.

use crate::dedup::SeenSet;
use crate::envelope::{Envelope, EnvelopeError};

#[derive(Debug, PartialEq, Eq)]
pub enum PreRender {
    /// Fresh, valid, timely: render it, then consult `post_render`.
    Render,
    /// Hub id already seen (redelivery after a crash between mark and
    /// ack): ack silently, never a duplicate toast (S6b).
    AckSilently,
    /// Valid but past the TTL client-side (claimed before a suspend,
    /// rendered after resume — the hub can only expire unclaimed
    /// messages): ack without rendering, logged as expired (AR5).
    AckStale,
    /// Structurally hopeless: nack dead=true so it lands visibly in
    /// the dead letters instead of retry-looping (M9).
    Poison(EnvelopeError),
}

pub fn pre_render(
    parsed: &Result<Envelope, EnvelopeError>,
    hub_id: &str,
    seen: &SeenSet,
    stale: bool,
) -> PreRender {
    match parsed {
        Err(e) => PreRender::Poison(e.clone()),
        Ok(_) if seen.contains(hub_id) => PreRender::AckSilently,
        Ok(_) if stale => PreRender::AckStale,
        Ok(_) => PreRender::Render,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PostRender {
    /// Mark seen, then ack — in that order: a crash between the two
    /// redelivers a marked id, which `pre_render` acks silently. The
    /// reverse order could lose the toast; this order at worst
    /// duplicates it, the accepted direction (AR5).
    MarkSeenThenAck,
    /// Transient render failure (non-zero exit, spawn error, AR19
    /// child timeout): nack without dead, the hub redelivers (K3).
    Nack,
}

pub fn post_render(render_succeeded: bool) -> PostRender {
    if render_succeeded {
        PostRender::MarkSeenThenAck
    } else {
        PostRender::Nack
    }
}

/// AR5's "settle call rejected" row: what to do when the ack/nack HTTP
/// call itself answers. 4xx means the lease or message is gone (suspend
/// outlived the lease, message expired meanwhile) — settled, move on;
/// retry-looping a settle call is the undefined state the critic found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleCallOutcome {
    Settled,
    /// Gone at the hub side — log it, treat as settled, move on.
    GoneAnyway,
    /// Transport/5xx — worth a bounded retry; give up → lease expiry
    /// redelivers and dedup absorbs it.
    Retry,
}

pub fn settle_call_outcome(status: Option<u16>) -> SettleCallOutcome {
    match status {
        Some(s) if (200..300).contains(&s) => SettleCallOutcome::Settled,
        Some(s) if (400..500).contains(&s) => SettleCallOutcome::GoneAnyway,
        _ => SettleCallOutcome::Retry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::parse_envelope;

    fn valid() -> Result<Envelope, EnvelopeError> {
        parse_envelope(br#"{"v":1,"id":"payload-id","title":{"nl":"a"}}"#)
    }

    #[test]
    fn ar5_fresh_valid_timely_message_renders() {
        assert_eq!(
            pre_render(&valid(), "hub-1", &SeenSet::new(4), false),
            PreRender::Render
        );
    }

    #[test]
    fn ar5_s6b_a_seen_hub_id_acks_without_rendering() {
        let mut seen = SeenSet::new(4);
        seen.insert("hub-1");
        assert_eq!(
            pre_render(&valid(), "hub-1", &seen, false),
            PreRender::AckSilently
        );
    }

    #[test]
    fn ar5_the_key_is_the_hub_id_not_the_payload_id() {
        let mut seen = SeenSet::new(4);
        seen.insert("payload-id");
        // The payload id being "seen" is irrelevant; only hub ids count.
        assert_eq!(
            pre_render(&valid(), "hub-2", &seen, false),
            PreRender::Render
        );
    }

    #[test]
    fn ar5_a_stale_message_is_acked_unrendered() {
        assert_eq!(
            pre_render(&valid(), "hub-1", &SeenSet::new(4), true),
            PreRender::AckStale
        );
    }

    #[test]
    fn ar5_m9_a_parse_failure_is_poison_even_when_stale() {
        let parsed = parse_envelope(b"garbage");
        assert!(matches!(
            pre_render(&parsed, "hub-1", &SeenSet::new(4), true),
            PreRender::Poison(EnvelopeError::Malformed(_))
        ));
    }

    #[test]
    fn ar5_render_success_marks_then_acks() {
        assert_eq!(post_render(true), PostRender::MarkSeenThenAck);
    }

    #[test]
    fn ar5_k3_render_failure_nacks_for_redelivery() {
        assert_eq!(post_render(false), PostRender::Nack);
    }

    #[test]
    fn ar5_a_rejected_settle_call_is_never_retry_looped() {
        assert_eq!(settle_call_outcome(Some(200)), SettleCallOutcome::Settled);
        assert_eq!(
            settle_call_outcome(Some(409)),
            SettleCallOutcome::GoneAnyway
        );
        assert_eq!(
            settle_call_outcome(Some(404)),
            SettleCallOutcome::GoneAnyway
        );
        assert_eq!(settle_call_outcome(Some(500)), SettleCallOutcome::Retry);
        assert_eq!(settle_call_outcome(None), SettleCallOutcome::Retry);
    }
}
