use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::peer_session::SessionId;

#[derive(Debug)]
pub(super) struct RecentSessionIds {
    entries: HashMap<SessionId, Instant>,
    max_age: Duration,
    capacity: usize,
}

impl RecentSessionIds {
    pub(super) fn new(max_age: Duration, capacity: usize) -> Self {
        Self { entries: HashMap::new(), max_age, capacity: capacity.max(1) }
    }

    pub(super) fn seen_or_remember(&mut self, session_id: SessionId, now: Instant) -> bool {
        self.prune(now);
        if self.entries.contains_key(&session_id) {
            return true;
        }
        if self.entries.len() >= self.capacity {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, inserted_at)| **inserted_at)
                .map(|(session_id, _)| *session_id);
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(session_id, now);
        false
    }

    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|_, inserted_at| now.saturating_duration_since(*inserted_at) <= self.max_age);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_capacity_bounded() {
        let now = Instant::now();
        let mut recent = RecentSessionIds::new(Duration::from_secs(60), 2);
        let first = SessionId::new();
        let second = SessionId::new();
        let third = SessionId::new();

        assert!(!recent.seen_or_remember(first, now));
        assert!(!recent.seen_or_remember(second, now + Duration::from_millis(1)));
        assert!(!recent.seen_or_remember(third, now + Duration::from_millis(2)));
        assert_eq!(recent.entries.len(), 2);
        assert!(!recent.entries.contains_key(&first));
    }
}
