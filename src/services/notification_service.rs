use std::{
    collections::HashMap,
    sync::atomic::AtomicU64,
    time::{Duration, Instant},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const INFO_DEFAULT_DURATION: Duration = Duration::from_secs(7);
const ERROR_DEFAULT_DURATION: Duration = Duration::from_secs(10);
const SUCCESS_DEFAULT_DURATION: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub kind: NotificationKind,
    pub created_at: Instant,
    pub duration: Duration,
}

impl Notification {
    pub fn new(id: u64, message: String, kind: NotificationKind) -> Self {
        Self {
            id,
            message,
            kind,
            created_at: Instant::now(),
            duration: match kind {
                NotificationKind::Info => INFO_DEFAULT_DURATION,
                NotificationKind::Error => ERROR_DEFAULT_DURATION,
                NotificationKind::Success => SUCCESS_DEFAULT_DURATION,
            },
        }
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.created_at) > self.duration
    }
}

pub struct NotificationService {
    notifications: HashMap<u64, Notification>,
}

impl NotificationService {
    pub fn new() -> Self {
        Self { notifications: HashMap::new() }
    }

    pub fn error(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Error));
    }

    pub fn info(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Info));
    }

    pub fn success(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Success));
    }

    pub fn dismiss(&mut self, id: u64) {
        self.notifications.remove(&id);
    }

    pub fn dismiss_expired(&mut self, now: Instant) {
        self.notifications.retain(|_k, n| !n.expired(now));
    }

    pub fn notifications(&self) -> impl Iterator<Item = &Notification> {
        self.notifications.values()
    }
}
