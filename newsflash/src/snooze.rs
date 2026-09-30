//! feat-11: Snooze puts the notification back on the hub at once, due
//! `snooze_minutes` later (Kenny, 2026-09-30: "direct op de queue zetten,
//! maar dat het pas binnen 15 minuten triggert"). Until then a click on
//! Snooze only reached `notify.actions`, which nothing reads.
//!
//! It goes to its own topic (`snooze_topic`, default `<topic>.snooze`),
//! not back to `notify.kenny`: Home Assistant's `ha` subscription reads
//! that one too and would ring the phone and the lights a second time.
//! Only newsflash's `desktop` subscription reads the snooze topic, so a
//! snooze made on Windows arrives on Garuda after a reboot, and survives
//! a restart of the hub (kyu keeps the due time in its store, W4).
//! The snooze topic has no TTL: kyu counts a TTL from the publish time,
//! so a 10-minute TTL would expire a 15-minute snooze before it is due.

use courier_core::hub::{HubMessage, HubPayload};
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
    topic: String,
    recent: VecDeque<HubMessage>,
}

static BOOK: Mutex<Book> = Mutex::new(Book {
    minutes: DEFAULT_SNOOZE_MINUTES,
    topic: String::new(),
    recent: VecDeque::new(),
});

/// What a Snooze click publishes, and where.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub topic: String,
    pub body: String,
    pub minutes: u32,
}

impl Plan {
    pub fn delay_ms(&self) -> u64 {
        u64::from(self.minutes) * 60_000
    }
}

pub fn default_topic(topic: &str) -> String {
    format!("{topic}.snooze")
}

pub fn configure(minutes: u32, topic: &str) {
    if let Ok(mut b) = BOOK.lock() {
        b.minutes = minutes;
        b.topic = topic.to_string();
    }
}

/// Called for every shown message, so a later Snooze click can find it.
pub fn remember(message: &HubMessage) {
    if let Ok(mut b) = BOOK.lock() {
        b.recent.retain(|m| m.id != message.id);
        if b.recent.len() == RECENT {
            b.recent.pop_front();
        }
        b.recent.push_back(message.clone());
    }
}

/// What a Snooze click on the toast of `hub_id` publishes: the original
/// envelope and the delay. `minutes` overrides the config (a
/// `snooze_minutes` input on the toast). `None` when the message is no
/// longer known (shown before a restart).
pub fn plan(hub_id: &str, minutes: Option<u32>) -> Option<Plan> {
    let b = BOOK.lock().ok()?;
    let minutes = minutes
        .filter(|m| (1..=MAX_SNOOZE_MINUTES).contains(m))
        .unwrap_or(b.minutes);
    let message = b.recent.iter().find(|m| m.id == hub_id)?;
    let HubPayload::Json(value) = &message.payload else {
        return None;
    };
    Some(Plan {
        topic: b.topic.clone(),
        body: value.to_string(),
        minutes,
    })
}

/// A Snooze click: publish the original envelope to the snooze topic,
/// due later. Logged either way; a failure leaves the notification in
/// the history, where it already is.
pub fn snooze_click(client: &crate::hub_client::HubClient, hub_id: &str, minutes: Option<u32>) {
    let Some(plan) = plan(hub_id, minutes) else {
        crate::logx::warn(&format!(
            "{hub_id}: snooze clicked, but this courier did not show that message (shown \
             before a restart?) — it stays in the notification history"
        ));
        return;
    };
    match client.publish_delayed(&plan.topic, &plan.body, plan.delay_ms()) {
        Ok(id) => crate::logx::info(&format!(
            "{hub_id}: snoozed — back on the hub as {id} on {}, due in {} min",
            plan.topic, plan.minutes
        )),
        Err(e) => crate::logx::warn(&format!(
            "{hub_id}: snooze could not reach the hub ({}) — it stays in the notification history",
            e.detail
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feat_11_a_snooze_republishes_the_original_envelope_with_its_minutes() {
        let payload = serde_json::json!({"v":1,"id":"p","title":{"nl":"a"}});
        let msg = HubMessage {
            id: "hub-snooze-1".into(),
            attempt: 1,
            published_at_ms: 0,
            payload: HubPayload::Json(payload.clone()),
        };
        configure(15, "notify.kenny.snooze");
        assert_eq!(
            plan("hub-snooze-1", None),
            None,
            "unknown before it was shown"
        );
        remember(&msg);
        let p = plan("hub-snooze-1", None).unwrap();
        assert_eq!(
            (p.topic.as_str(), p.minutes, p.delay_ms()),
            ("notify.kenny.snooze", 15, 900_000)
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&p.body).unwrap(),
            payload
        );
        assert_eq!(
            plan("hub-snooze-1", Some(5)).unwrap().minutes,
            5,
            "toast input wins"
        );
        assert_eq!(
            plan("hub-snooze-1", Some(0)).unwrap().minutes,
            15,
            "out of range: config"
        );
        assert_eq!(default_topic("notify.kenny"), "notify.kenny.snooze");
    }
}
