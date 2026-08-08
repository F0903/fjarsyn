use std::time::Duration;

use tokio::time::Instant;

/// Application-owned timing information supplied to service shutdown.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShutdownContext {
    deadline: Option<Instant>,
}

impl ShutdownContext {
    pub const fn new(deadline: Option<Instant>) -> Self {
        Self { deadline }
    }

    pub const fn deadline(self) -> Option<Instant> {
        self.deadline
    }

    /// Returns the earlier of the application deadline and a service's own
    /// relative shutdown limit.
    pub fn bounded_deadline(self, relative_timeout: Duration) -> Instant {
        let relative_deadline = Instant::now() + relative_timeout;
        self.deadline.map_or(relative_deadline, |deadline| deadline.min(relative_deadline))
    }
}
