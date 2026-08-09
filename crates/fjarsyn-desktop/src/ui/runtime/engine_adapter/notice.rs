use fjarsyn_engine::{
    identity::PeerId, messaging, peer_session, peer_session::CloseReason, screen_share,
};
use tokio::sync::mpsc;

use super::failure::{Failure, Source};

/// Transient engine semantics that the desktop intentionally presents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum Notice {
    IncomingRequest { peer_id: PeerId },
    Connected { peer_id: PeerId },
    Closed { peer_id: PeerId, reason: CloseReason },
    IncomingMessage { peer_id: PeerId },
    ScreenShareFailed { reason: String },
}

impl Notice {
    pub(super) fn from_session(event: peer_session::Event) -> Option<Self> {
        match event {
            peer_session::Event::IncomingRequest { peer_id, .. } => {
                Some(Self::IncomingRequest { peer_id })
            }
            peer_session::Event::Connected { peer_id, .. } => Some(Self::Connected { peer_id }),
            peer_session::Event::Closed {
                reason: peer_session::CloseReason::LocalDisconnect,
                ..
            } => None,
            peer_session::Event::Closed { peer_id, reason, .. } => {
                Some(Self::Closed { peer_id, reason })
            }
            peer_session::Event::MessageSent { .. }
            | peer_session::Event::MessageReceived { .. }
            | peer_session::Event::MessageReceiptReceived { .. }
            | peer_session::Event::LocalShareChanged { .. }
            | peer_session::Event::RemoteShareChanged { .. } => None,
        }
    }

    pub(super) fn from_messaging(event: messaging::Event) -> Option<Self> {
        match event {
            messaging::Event::IncomingMessage { peer_id, .. } => {
                Some(Self::IncomingMessage { peer_id })
            }
            messaging::Event::ConversationUpdated { .. }
            | messaging::Event::MessageStatusChanged { .. } => None,
        }
    }

    pub(super) fn from_screen_share(event: screen_share::Event) -> Option<Self> {
        match event {
            screen_share::Event::LocalFailed { reason, .. }
            | screen_share::Event::RemoteFailed { reason, .. } => {
                Some(Self::ScreenShareFailed { reason })
            }
            screen_share::Event::CodecRestartRequired { .. } => None,
        }
    }
}

/// Lossy bounded publisher for presentation-only notices.
pub(super) struct Publisher {
    tx: mpsc::Sender<Notice>,
}

impl Publisher {
    pub(super) const fn new(tx: mpsc::Sender<Notice>) -> Self {
        Self { tx }
    }

    pub(super) fn publish(&self, notice: Notice, source: Source) -> Result<(), Failure> {
        match self.tx.try_send(notice) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(%source, "dropping a transient desktop notice because its queue is full");
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(Failure::runtime_channel_closed(source))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::{
        identity::PeerId,
        messaging,
        peer_session::{self, LocalShareState, MessageId, SessionId},
        screen_share,
    };
    use tokio::sync::mpsc;

    use super::{super::failure::Source, Notice, Publisher};

    #[test]
    fn session_events_keep_only_presented_lifecycle_notices() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();

        assert_eq!(
            Notice::from_session(peer_session::Event::IncomingRequest {
                session_id,
                peer_id: peer_id.clone(),
            }),
            Some(Notice::IncomingRequest { peer_id: peer_id.clone() })
        );
        assert!(
            Notice::from_session(peer_session::Event::LocalShareChanged {
                session_id,
                peer_id,
                state: LocalShareState::Inactive,
            })
            .is_none()
        );
    }

    #[test]
    fn local_disconnect_and_duplicate_durable_events_are_not_forwarded() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();

        assert!(
            Notice::from_session(peer_session::Event::Closed {
                session_id,
                peer_id: peer_id.clone(),
                reason: peer_session::CloseReason::LocalDisconnect,
            })
            .is_none()
        );
        assert!(
            Notice::from_messaging(messaging::Event::ConversationUpdated { peer_id }).is_none()
        );
        assert!(
            Notice::from_screen_share(screen_share::Event::CodecRestartRequired {
                direction: screen_share::CodecDirection::Encoder,
            })
            .is_none()
        );
    }

    #[test]
    fn incoming_message_notice_drops_body_and_identifiers() {
        let peer_id = PeerId::new("peer").unwrap();

        let notice = Notice::from_messaging(messaging::Event::IncomingMessage {
            session_id: SessionId::new(),
            peer_id: peer_id.clone(),
            message_id: MessageId::new(),
            body: "sensitive body".into(),
        });

        assert_eq!(notice, Some(Notice::IncomingMessage { peer_id }));
    }

    #[test]
    fn notice_overflow_is_lossy_and_never_blocks_the_coordinator() {
        let (tx, mut rx) = mpsc::channel(1);
        let publisher = Publisher::new(tx);
        let first = Notice::IncomingMessage { peer_id: PeerId::new("first").unwrap() };
        let second = Notice::IncomingMessage { peer_id: PeerId::new("second").unwrap() };

        publisher.publish(first.clone(), Source::MessagingEvents).unwrap();
        publisher.publish(second, Source::MessagingEvents).unwrap();

        assert_eq!(rx.try_recv().unwrap(), first);
        assert!(rx.try_recv().is_err());
    }
}
