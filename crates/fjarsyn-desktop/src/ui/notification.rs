//! Desktop notifications and expiration-driven cleanup.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const INFO_DEFAULT_DURATION: Duration = Duration::from_secs(7);
const ERROR_DEFAULT_DURATION: Duration = Duration::from_secs(10);
const SUCCESS_DEFAULT_DURATION: Duration = Duration::from_secs(5);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum Kind {
    Info,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct Notification {
    pub(in crate::ui) id: u64,
    pub(in crate::ui) message: String,
    pub(in crate::ui) kind: Kind,
    created_at: Instant,
    duration: Duration,
}

impl Notification {
    fn new(id: u64, message: String, kind: Kind) -> Self {
        Self {
            id,
            message,
            kind,
            created_at: Instant::now(),
            duration: match kind {
                Kind::Info => INFO_DEFAULT_DURATION,
                Kind::Error => ERROR_DEFAULT_DURATION,
                Kind::Success => SUCCESS_DEFAULT_DURATION,
            },
        }
    }

    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.created_at) >= self.duration
    }

    fn deadline(&self) -> Instant {
        self.created_at + self.duration
    }
}

pub(in crate::ui) struct Center {
    notifications: HashMap<u64, Notification>,
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Center {
    pub(in crate::ui) fn new() -> Self {
        Self { notifications: HashMap::new() }
    }

    pub(in crate::ui) fn error(&mut self, message: impl Into<String>) {
        self.notify(message.into(), Kind::Error);
    }

    pub(in crate::ui) fn info(&mut self, message: impl Into<String>) {
        self.notify(message.into(), Kind::Info);
    }

    pub(in crate::ui) fn success(&mut self, message: impl Into<String>) {
        self.notify(message.into(), Kind::Success);
    }

    pub(in crate::ui) fn dismiss(&mut self, id: u64) {
        self.notifications.remove(&id);
    }

    pub(in crate::ui) fn dismiss_expired(&mut self, now: Instant) {
        self.notifications.retain(|_, notification| !notification.expired(now));
    }

    pub(in crate::ui) fn next_deadline(&self) -> Option<Instant> {
        self.notifications.values().map(Notification::deadline).min()
    }

    pub(in crate::ui) fn notifications(&self) -> impl Iterator<Item = &Notification> {
        self.notifications.values()
    }

    fn notify(&mut self, message: String, kind: Kind) {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.notifications.insert(id, Notification::new(id, message, kind));
    }
}
