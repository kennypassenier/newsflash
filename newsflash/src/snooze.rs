//! feat-11: the Snooze button shows the notification again later
//! (Kenny, 2026-09-30: "15 minuten"). Until then a click on Snooze only
//! reached `notify.actions`, which nothing reads.
//!
//! Local on purpose: the hub copy was acked when it was shown, and
//! re-publishing it would ring the phone and the lights a second time.
//! The courier keeps the last shown messages in memory; a snooze moves
//! one to the due list, and the loop shows it again once it is due.
//! In memory only: a restart forgets pending snoozes (the notification
//! stays in the history).

use courier_core::envelope::Envelope;
use courier_core::hub::HubMessage;
use std::collections::VecDeque;
use std::sync::Mutex;

/// The button id pipeline-v2's contract reserves for "remind me later".
pub const SNOOZE_ACTION: &str = "snooze";
pub const DEFAULT_SNOOZE_MINUTES: u32 = 15;
/// A day: past that a snooze is a config typo, not a reminder.
pub const MAX_SNOOZE_MINUTES: u32 = 24 * 60;
/// Shown messages kept for a later snooze click; older ones cannot be
/// snoozed any more (their toast is long gone anyway).
const RECENT: usize = 64;

struct Book {
    minutes: u32,
    recent: VecDeque<(HubMessage, Envelope)>,
    due: Vec<(u64, HubMessage, Envelope)>,
}

static BOOK: Mutex<Book> = Mutex::new(Book {
    minutes: DEFAULT_SNOOZE_MINUTES,
    recent: VecDeque::new(),
    due: Vec::new(),
});

pub fn set_minutes(minutes: u32) {
    if let Ok(mut b) = BOOK.lock() {
        b.minutes = minutes;
    }
}

/// Called for every shown message, so a later Snooze click can find it.
pub fn remember(message: &HubMessage, env: &Envelope) {
    if let Ok(mut b) = BOOK.lock() {
        b.recent.retain(|(m, _)| m.id != message.id);
        if b.recent.len() == RECENT {
            b.recent.pop_front();
        }
        b.recent.push_back((message.clone(), env.clone()));
    }
}

/// A Snooze click on the toast of `hub_id`. `minutes` overrides the
/// config (a `snooze_minutes` input on the toast). Returns the minutes
/// used, or `None` when the message is no longer known.
pub fn snooze(hub_id: &str, minutes: Option<u32>, now_ms: u64) -> Option<u32> {
    let mut b = BOOK.lock().ok()?;
    let minutes = minutes
        .filter(|m| (1..=MAX_SNOOZE_MINUTES).contains(m))
        .unwrap_or(b.minutes);
    let (message, env) = b.recent.iter().find(|(m, _)| m.id == hub_id)?.clone();
    b.due.retain(|(_, m, _)| m.id != hub_id);
    b.due
        .push((now_ms + u64::from(minutes) * 60_000, message, env));
    Some(minutes)
}

/// Everything whose snooze has run out, removed from the due list.
pub fn take_due(now_ms: u64) -> Vec<(HubMessage, Envelope)> {
    let Ok(mut b) = BOOK.lock() else {
        return Vec::new();
    };
    let (due, later): (Vec<_>, Vec<_>) = b.due.drain(..).partition(|(at, _, _)| *at <= now_ms);
    b.due = later;
    due.into_iter().map(|(_, m, e)| (m, e)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use courier_core::envelope::parse_envelope;

    #[test]
    fn feat_11_a_snoozed_message_comes_back_after_its_minutes_and_only_then() {
        let msg = HubMessage {
            id: "hub-snooze-1".into(),
            attempt: 1,
            published_at_ms: 0,
            payload: courier_core::hub::HubPayload::Binary,
        };
        let env = parse_envelope(br#"{"v":1,"id":"p","title":{"nl":"a"}}"#).unwrap();
        set_minutes(15);
        assert_eq!(
            snooze("hub-snooze-1", None, 0),
            None,
            "unknown before it was shown"
        );
        remember(&msg, &env);
        assert_eq!(snooze("hub-snooze-1", None, 1_000), Some(15));
        assert!(take_due(1_000 + 14 * 60_000).is_empty());
        let due = take_due(1_000 + 15 * 60_000);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0.id, "hub-snooze-1");
        assert!(take_due(u64::MAX).is_empty(), "shown once, not again");
        assert_eq!(
            snooze("hub-snooze-1", Some(5), 0),
            Some(5),
            "toast input wins"
        );
    }
}
