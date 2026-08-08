use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fjarsyn_engine::{messaging, peer_session, presence, screen_share};

#[derive(Debug, Clone)]
pub(in crate::ui) enum Event {
    Presence(presence::Snapshot),
    Sessions(peer_session::Snapshot),
    SessionChange(peer_session::Event),
    Messaging(messaging::Snapshot),
    MessagingChange(messaging::Event),
    ScreenShareSnapshotReady(ScreenShareUpdate),
    ScreenShareChange(screen_share::Event),
}

/// Coalesces high-rate screen-share snapshots behind one lightweight UI event.
///
/// `replace` and `take_latest` update the latest value and pending-event flag
/// under the same lock. An update racing with UI consumption therefore either
/// becomes the value consumed by the current event or schedules the next one.
#[derive(Clone)]
pub(in crate::ui) struct ScreenShareUpdate {
    state: Arc<Mutex<ScreenShareUpdateState>>,
    generation: u64,
}

#[derive(Clone, Default)]
pub(in crate::ui::runtime) struct ScreenShareUpdateSlot {
    state: Arc<Mutex<ScreenShareUpdateState>>,
}

#[derive(Default)]
struct ScreenShareUpdateState {
    latest: Option<screen_share::Snapshot>,
    notification_pending: bool,
    generation: u64,
}

impl ScreenShareUpdateSlot {
    /// Replaces the queued value and returns a notification when one must be sent.
    pub(in crate::ui::runtime) fn replace(
        &self,
        snapshot: screen_share::Snapshot,
    ) -> Option<ScreenShareUpdate> {
        let mut state = lock_update_state(&self.state);
        state.latest = Some(snapshot);
        if state.notification_pending {
            None
        } else {
            state.notification_pending = true;
            state.generation = state.generation.wrapping_add(1);
            Some(ScreenShareUpdate { state: self.state.clone(), generation: state.generation })
        }
    }
}

impl ScreenShareUpdate {
    /// Takes the newest value and makes a future update eligible to notify.
    pub(in crate::ui) fn take_latest(&self) -> Option<screen_share::Snapshot> {
        let mut state = lock_update_state(&self.state);
        if !state.notification_pending || state.generation != self.generation {
            return None;
        }
        let latest = state.latest.take();
        state.notification_pending = false;
        latest
    }
}

fn lock_update_state(
    state: &Mutex<ScreenShareUpdateState>,
) -> MutexGuard<'_, ScreenShareUpdateState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl std::fmt::Debug for ScreenShareUpdate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ScreenShareUpdate").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::screen_share;

    use super::ScreenShareUpdateSlot;

    #[test]
    fn only_one_notification_is_pending_while_the_latest_value_is_replaced() {
        let updates = ScreenShareUpdateSlot::default();

        let first = updates.replace(screen_share::Snapshot::default()).unwrap();
        assert!(updates.replace(screen_share::Snapshot::default()).is_none());
        assert!(first.take_latest().is_some());

        let second = updates.replace(screen_share::Snapshot::default()).unwrap();
        assert!(first.take_latest().is_none());
        assert!(second.take_latest().is_some());
    }
}
