use std::time::{Duration, Instant};

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
    pub(super) fn new(id: u64, message: String, kind: NotificationKind) -> Self {
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
