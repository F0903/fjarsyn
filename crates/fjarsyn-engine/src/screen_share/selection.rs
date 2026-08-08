use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::Notify;

use crate::peer_session::SessionId;

/// Opaque identity of one capture-selection and local-share start attempt.
///
/// A token is valid only for the exact service-wide reservation that created
/// it. Reusing an older token cannot affect a newer picker or share attempt.
/// Dropping its last clone before a start succeeds cancels that reservation.
#[derive(Clone)]
pub struct Selection {
    key: SelectionKey,
    lease: Arc<Lease>,
}

#[derive(Clone)]
pub(super) struct SelectionKey {
    session_id: SessionId,
    cancellation: Arc<Cancellation>,
}

struct Lease {
    key: SelectionKey,
    committed: AtomicBool,
}

struct Cancellation {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Selection {
    pub(super) fn new(session_id: SessionId) -> Self {
        let key = SelectionKey {
            session_id,
            cancellation: Arc::new(Cancellation {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        };
        Self { key: key.clone(), lease: Arc::new(Lease { key, committed: AtomicBool::new(false) }) }
    }

    pub const fn session_id(&self) -> SessionId {
        self.key.session_id
    }

    pub(super) fn cancel(&self) {
        self.key.cancel();
    }

    pub(super) fn key(&self) -> SelectionKey {
        self.key.clone()
    }

    pub(super) fn commit(&self) {
        self.lease.committed.store(true, Ordering::Release);
    }
}

impl SelectionKey {
    pub(super) const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(super) fn cancel(&self) {
        if !self.cancellation.cancelled.swap(true, Ordering::AcqRel) {
            self.cancellation.notify.notify_waiters();
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancellation.cancelled.load(Ordering::Acquire)
    }

    pub(super) async fn cancelled(&self) {
        let notified = self.cancellation.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_cancelled() {
            notified.await;
        }
    }
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Selection {}

impl std::fmt::Debug for Selection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Selection")
            .field("session_id", &self.key.session_id)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SelectionKey {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id && Arc::ptr_eq(&self.cancellation, &other.cancellation)
    }
}

impl Eq for SelectionKey {}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.committed.load(Ordering::Acquire) {
            self.key.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Selection;
    use crate::peer_session::SessionId;

    #[test]
    fn separately_created_tokens_never_compare_equal() {
        let session_id = SessionId::new();
        let first = Selection::new(session_id);
        let clone = first.clone();
        let second = Selection::new(session_id);

        assert_eq!(first, clone);
        assert_ne!(first, second);
    }

    #[test]
    fn dropping_the_last_uncommitted_lease_cancels_its_actor_identity() {
        let selection = Selection::new(SessionId::new());
        let key = selection.key();

        drop(selection);

        assert!(key.is_cancelled());
    }

    #[test]
    fn a_committed_lease_does_not_cancel_its_actor_identity() {
        let selection = Selection::new(SessionId::new());
        let key = selection.key();
        selection.commit();

        drop(selection);

        assert!(!key.is_cancelled());
    }
}
