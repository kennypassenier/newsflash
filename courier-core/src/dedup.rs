//! Bounded seen-set for redelivery dedup (K4, AR6). Pure bookkeeping:
//! the shell owns the file; core owns the shape and the JSON codec, so
//! corruption handling is testable without a filesystem.

use std::collections::{HashSet, VecDeque};

pub const DEFAULT_CAPACITY: usize = 512;

#[derive(Debug)]
pub struct SeenSet {
    cap: usize,
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl SeenSet {
    pub fn new(cap: usize) -> Self {
        SeenSet {
            cap: cap.max(1),
            order: VecDeque::new(),
            set: HashSet::new(),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    /// Insert, evicting the oldest id past capacity. Re-inserting a
    /// known id is a no-op (it keeps its original age — good enough at
    /// 512 ≫ any 10-minute backlog).
    pub fn insert(&mut self, id: &str) {
        if self.set.contains(id) {
            return;
        }
        self.order.push_back(id.to_owned());
        self.set.insert(id.to_owned());
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
    }

    pub fn to_json(&self) -> String {
        // A plain array, oldest first — trivially inspectable on disk.
        serde_json::to_string(&self.order).expect("a string list always serializes")
    }

    /// `None` on corrupt input: the caller starts empty and logs it —
    /// fail-open, because an empty seen-set costs at worst a duplicate
    /// toast, never a lost one (AR6).
    pub fn from_json(json: &str, cap: usize) -> Option<Self> {
        let ids: Vec<String> = serde_json::from_str(json).ok()?;
        let mut seen = SeenSet::new(cap);
        for id in &ids {
            seen.insert(id);
        }
        Some(seen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k4_a_seen_id_is_recognized() {
        let mut s = SeenSet::new(4);
        s.insert("a");
        assert!(s.contains("a"));
        assert!(!s.contains("b"));
    }

    #[test]
    fn k4_capacity_evicts_oldest_first() {
        let mut s = SeenSet::new(2);
        s.insert("a");
        s.insert("b");
        s.insert("c");
        assert!(!s.contains("a"));
        assert!(s.contains("b") && s.contains("c"));
    }

    #[test]
    fn k4_round_trips_through_json() {
        let mut s = SeenSet::new(8);
        s.insert("a");
        s.insert("b");
        let restored = SeenSet::from_json(&s.to_json(), 8).unwrap();
        assert!(restored.contains("a") && restored.contains("b"));
    }

    #[test]
    fn ar6_corrupt_json_yields_none_for_a_logged_empty_start() {
        assert!(SeenSet::from_json("{not json", 8).is_none());
        assert!(SeenSet::from_json(r#"{"wrong":"shape"}"#, 8).is_none());
    }

    #[test]
    fn ar6_restore_respects_a_smaller_capacity() {
        let mut s = SeenSet::new(10);
        for i in 0..10 {
            s.insert(&format!("id{i}"));
        }
        let restored = SeenSet::from_json(&s.to_json(), 3).unwrap();
        assert!(!restored.contains("id0"));
        assert!(restored.contains("id9"));
    }
}
