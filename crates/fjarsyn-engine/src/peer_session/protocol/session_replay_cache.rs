use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::peer_session::Error;

#[derive(Debug)]
pub(in crate::peer_session) struct SessionReplayCache {
    seen: HashMap<Uuid, DateTime<Utc>>,
    capacity: usize,
}

impl SessionReplayCache {
    pub(in crate::peer_session) fn new(capacity: usize) -> Self {
        Self { seen: HashMap::with_capacity(capacity.min(4096)), capacity: capacity.max(1) }
    }

    pub(super) fn remember(
        &mut self,
        message_id: Uuid,
        created_at: DateTime<Utc>,
        now: DateTime<Utc>,
        max_age: Duration,
    ) -> Result<(), Error> {
        let oldest_allowed = now - max_age;
        self.seen.retain(|_, timestamp| *timestamp >= oldest_allowed);
        if self.seen.contains_key(&message_id) {
            return Err(Error::Protocol("signaling replay detected".into()));
        }
        if self.seen.len() >= self.capacity {
            let oldest = self
                .seen
                .iter()
                .min_by_key(|(_, timestamp)| **timestamp)
                .map(|(message_id, _)| *message_id);
            if let Some(oldest) = oldest {
                self.seen.remove(&oldest);
            }
        }
        self.seen.insert(message_id, created_at);
        Ok(())
    }
}
