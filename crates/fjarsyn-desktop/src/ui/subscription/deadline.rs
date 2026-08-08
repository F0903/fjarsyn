use futures::stream::once;
use iced::Subscription;

use crate::ui::message::Message;

#[derive(Clone, Copy)]
struct DeadlineSubscriptionData {
    deadline: std::time::Instant,
    since_start: std::time::Duration,
}

impl std::hash::Hash for DeadlineSubscriptionData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.since_start.hash(state);
    }
}

impl PartialEq for DeadlineSubscriptionData {
    fn eq(&self, other: &Self) -> bool {
        self.since_start == other.since_start
    }
}

impl Eq for DeadlineSubscriptionData {}

pub(super) fn deadline_subscription(
    started_at: std::time::Instant,
    deadline: std::time::Instant,
) -> Subscription<Message> {
    Subscription::run_with(
        DeadlineSubscriptionData {
            deadline,
            since_start: deadline.saturating_duration_since(started_at),
        },
        |data| {
            let deadline = data.deadline;
            once(async move {
                let deadline = tokio::time::Instant::from_std(deadline);
                let now = tokio::time::Instant::now();

                if deadline > now {
                    tokio::time::sleep_until(deadline).await;
                }

                Message::Tick(deadline.into_std())
            })
        },
    )
}
