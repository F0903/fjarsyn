use tokio::sync::watch;

/// Retains the newest value from a watch channel for synchronous UI reads.
///
/// A separate receiver clone drives the asynchronous Iced subscription. The
/// subscription emits only a wake-up message; the UI then reads the newest
/// value from this receiver, naturally coalescing intermediate updates.
#[derive(Debug, Clone)]
pub(in crate::ui) struct Retained<T> {
    receiver: watch::Receiver<T>,
}

impl<T> Retained<T> {
    pub(in crate::ui::runtime) const fn new(receiver: watch::Receiver<T>) -> Self {
        Self { receiver }
    }

    /// Returns an independently versioned receiver for change notifications.
    pub(in crate::ui) fn subscribe(&self) -> watch::Receiver<T> {
        self.receiver.clone()
    }
}

impl<T: Clone> Retained<T> {
    /// Reads and marks the newest value as seen.
    ///
    /// This is deliberately unconditional: a sender may close after emitting a
    /// wake, and `has_changed` reports closure before exposing that final value.
    pub(in crate::ui) fn latest(&mut self) -> T {
        self.receiver.borrow_and_update().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Retained;

    #[tokio::test]
    async fn a_post_hydration_wake_reads_the_latest_coalesced_value() {
        let (sender, receiver) = tokio::sync::watch::channel(0);
        let mut retained = Retained::new(receiver);
        assert_eq!(retained.latest(), 0);
        let mut changes = retained.subscribe();

        sender.send(1).unwrap();
        sender.send(2).unwrap();
        sender.send(3).unwrap();

        changes.changed().await.unwrap();
        assert_eq!(retained.latest(), 3);
        assert!(!changes.has_changed().unwrap());
    }

    #[tokio::test]
    async fn an_update_before_subscription_creation_still_wakes_it() {
        let (sender, receiver) = tokio::sync::watch::channel("initial");
        let mut retained = Retained::new(receiver);
        assert_eq!(retained.latest(), "initial");

        sender.send("updated").unwrap();
        let mut changes = retained.subscribe();

        changes.changed().await.unwrap();
        assert_eq!(retained.latest(), "updated");
    }

    #[test]
    fn duplicate_wake_reapplies_the_same_retained_value() {
        let (sender, receiver) = tokio::sync::watch::channel("initial");
        let mut retained = Retained::new(receiver);

        sender.send("current").unwrap();

        assert_eq!(retained.latest(), "current");
        assert_eq!(retained.latest(), "current");
    }

    #[tokio::test]
    async fn final_value_remains_readable_after_the_sender_closes() {
        let (sender, receiver) = tokio::sync::watch::channel("initial");
        let mut retained = Retained::new(receiver);
        let mut changes = retained.subscribe();

        sender.send("final").unwrap();
        drop(sender);

        assert!(changes.changed().await.is_ok());
        assert_eq!(retained.latest(), "final");
        assert!(changes.changed().await.is_err());
    }
}
