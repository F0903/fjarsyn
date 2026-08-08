use std::sync::Arc;

use tokio::sync::{broadcast, watch};

use super::{CodecDirection, Event, LocalState, RemoteState, ShareBinding, Snapshot};
use crate::{media::frame::Frame, peer_session::SessionId};

#[derive(Debug, Clone)]
pub(super) enum Update {
    LocalState { session_id: SessionId, state: LocalState },
    RemoteState { session_id: SessionId, state: RemoteState },
    LocalFrame { session_id: SessionId, binding: ShareBinding, frame: Arc<Frame> },
    RemoteFrame { session_id: SessionId, binding: ShareBinding, frame: Arc<Frame> },
    CodecRestartRequired { direction: CodecDirection },
    SessionClosed { session_id: SessionId },
}

#[derive(Clone)]
pub(super) struct Output {
    snapshot_tx: watch::Sender<Snapshot>,
    event_tx: broadcast::Sender<Event>,
    session_snapshots: watch::Receiver<crate::peer_session::Snapshot>,
}

impl Output {
    pub(super) fn new(
        snapshot_tx: watch::Sender<Snapshot>,
        event_tx: broadcast::Sender<Event>,
        session_snapshots: watch::Receiver<crate::peer_session::Snapshot>,
    ) -> Self {
        Self { snapshot_tx, event_tx, session_snapshots }
    }

    pub(super) fn publish(&self, update: Update) {
        if let Some(session_id) = update_session(&update)
            && !self
                .session_snapshots
                .borrow()
                .session(session_id)
                .is_some_and(|session| super::retains_media_session(session.phase))
        {
            return;
        }
        let event = transient_event(&update);
        if matches!(&update, Update::LocalFrame { .. } | Update::RemoteFrame { .. }) {
            let sessions = self.session_snapshots.borrow().clone();
            if !frame_matches(&update, &sessions) {
                return;
            }
            self.snapshot_tx.send_modify(|snapshot| {
                let _ = snapshot.reconcile_shares(&sessions);
                snapshot.apply(update);
            });
        } else {
            self.snapshot_tx.send_modify(|snapshot| snapshot.apply(update));
        }
        if let Some(event) = event {
            let _ = self.event_tx.send(event);
        }
    }

    pub(super) fn reconcile_shares(&self, sessions: &crate::peer_session::Snapshot) {
        self.snapshot_tx.send_if_modified(|snapshot| snapshot.reconcile_shares(sessions));
    }
}

fn update_session(update: &Update) -> Option<SessionId> {
    match update {
        Update::LocalState { session_id, .. }
        | Update::RemoteState { session_id, .. }
        | Update::LocalFrame { session_id, .. }
        | Update::RemoteFrame { session_id, .. } => Some(*session_id),
        Update::CodecRestartRequired { .. } | Update::SessionClosed { .. } => None,
    }
}

fn frame_matches(update: &Update, snapshot: &crate::peer_session::Snapshot) -> bool {
    match update {
        Update::LocalFrame { session_id, binding, .. } => {
            snapshot.session(*session_id).is_some_and(|session| {
                super::retains_media_session(session.phase)
                    && match session.local_share {
                        crate::peer_session::LocalShareState::Active { share_id, epoch } => {
                            ShareBinding::new(share_id, epoch) == *binding
                        }
                        crate::peer_session::LocalShareState::Inactive => false,
                    }
            })
        }
        Update::RemoteFrame { session_id, binding, .. } => {
            snapshot.session(*session_id).is_some_and(|session| {
                super::retains_media_session(session.phase)
                    && match session.remote_share {
                        crate::peer_session::RemoteShareState::Active { share_id, epoch } => {
                            ShareBinding::new(share_id, epoch) == *binding
                        }
                        crate::peer_session::RemoteShareState::Inactive => false,
                    }
            })
        }
        _ => true,
    }
}

fn transient_event(update: &Update) -> Option<Event> {
    match update {
        Update::LocalState { session_id, state: LocalState::Failed(reason) } => {
            Some(Event::LocalFailed { session_id: *session_id, reason: reason.clone() })
        }
        Update::RemoteState { session_id, state: RemoteState::Failed(reason) } => {
            Some(Event::RemoteFailed { session_id: *session_id, reason: reason.clone() })
        }
        Update::CodecRestartRequired { direction } => {
            Some(Event::CodecRestartRequired { direction: *direction })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::{broadcast, watch};

    use super::{Output, ShareBinding, Snapshot, Update};
    use crate::{
        identity::PeerId,
        media::{
            Dimensions, PixelFormat,
            frame::{Frame, FrameData},
        },
        peer_session::{self, LocalShareState, RemoteShareState, SessionId, ShareEpoch, ShareId},
    };

    fn peer_snapshot(session_id: SessionId, binding: ShareBinding) -> peer_session::Snapshot {
        peer_snapshot_with_phase(session_id, binding, peer_session::Phase::Connected)
    }

    fn peer_snapshot_with_phase(
        session_id: SessionId,
        binding: ShareBinding,
        phase: peer_session::Phase,
    ) -> peer_session::Snapshot {
        peer_session::Snapshot {
            sessions: Arc::new(vec![peer_session::SessionSnapshot {
                session_id,
                peer_id: PeerId::new("peer-a").unwrap(),
                phase,
                local_share: LocalShareState::Inactive,
                remote_share: RemoteShareState::Active {
                    share_id: binding.share_id(),
                    epoch: binding.epoch(),
                },
            }]),
        }
    }

    fn frame() -> Arc<Frame> {
        Arc::new(Frame {
            data: FrameData::Software(bytes::Bytes::new()),
            format: PixelFormat::BGRA8,
            size: Dimensions::new(0, 0),
            duration: None,
        })
    }

    #[test]
    fn frame_publication_rejects_an_old_epoch_against_the_latest_peer_snapshot() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding_a = ShareBinding::new(share_id, ShareEpoch::FIRST);
        let binding_b = ShareBinding::new(
            share_id,
            ShareEpoch::try_from(ShareEpoch::FIRST.value() + 1).unwrap(),
        );
        let (_session_tx, session_rx) = watch::channel(peer_snapshot(session_id, binding_b));
        let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot::default());
        let (event_tx, _) = broadcast::channel(1);
        let output = Output::new(snapshot_tx, event_tx, session_rx);

        output.publish(Update::RemoteFrame { session_id, binding: binding_a, frame: frame() });
        assert!(snapshot_rx.borrow().session(session_id).remote_frame.is_none());

        output.publish(Update::RemoteFrame { session_id, binding: binding_b, frame: frame() });
        assert_eq!(snapshot_rx.borrow().session(session_id).remote_frame_binding, Some(binding_b));
    }

    #[test]
    fn late_updates_cannot_recreate_a_disconnecting_session() {
        let session_id = SessionId::new();
        let binding = ShareBinding::new(ShareId::new(), ShareEpoch::FIRST);
        let (_session_tx, session_rx) = watch::channel(peer_snapshot_with_phase(
            session_id,
            binding,
            peer_session::Phase::Disconnecting,
        ));
        let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot::default());
        let (event_tx, _) = broadcast::channel(1);
        let output = Output::new(snapshot_tx, event_tx, session_rx);

        output.publish(Update::RemoteState {
            session_id,
            state: crate::screen_share::RemoteState::Active,
        });
        output.publish(Update::RemoteFrame { session_id, binding, frame: frame() });

        assert!(!snapshot_rx.borrow().sessions().contains_key(&session_id));
    }
}
