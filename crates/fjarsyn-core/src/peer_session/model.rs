use std::{fmt, sync::Arc};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::PeerSessionError;
use crate::identity::PeerId;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn as_uuid(self) -> Uuid {
                self.0
            }

            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl std::str::FromStr for $name {
            type Err = PeerSessionError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self).map_err(|_| PeerSessionError::InvalidIdentifier {
                    kind: stringify!($name),
                    value: value.to_owned(),
                })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

uuid_id!(SessionId);
uuid_id!(MessageId);
uuid_id!(ShareId);

/// Monotonic media generation for one peer's screen shares within a session.
///
/// `ShareId` is the application identity presented to callers. `ShareEpoch` is
/// the compact, ordered media-plane boundary carried on RTP packets so a
/// receiver can distinguish delayed old media from early media for the next
/// share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShareEpoch(u64);

impl ShareEpoch {
    pub const FIRST: Self = Self(1);

    pub const fn value(self) -> u64 {
        self.0
    }

    pub(crate) const fn from_value(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn next(self) -> Result<Self, PeerSessionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| PeerSessionError::Protocol("screen-share epoch overflowed".into()))
    }

    pub(crate) fn require_valid(self) -> Result<(), PeerSessionError> {
        if self.0 == 0 {
            Err(PeerSessionError::Protocol("screen-share epoch must be non-zero".into()))
        } else {
            Ok(())
        }
    }
}

impl TryFrom<u64> for ShareEpoch {
    type Error = PeerSessionError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let epoch = Self(value);
        epoch.require_valid()?;
        Ok(epoch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeerSessionPhase {
    Requesting,
    Incoming,
    Negotiating,
    Connected,
    Reconnecting,
    Disconnecting,
}

impl PeerSessionPhase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Requesting => "requesting",
            Self::Incoming => "incoming",
            Self::Negotiating => "negotiating",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Disconnecting => "disconnecting",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalShareState {
    #[default]
    Inactive,
    Active {
        share_id: ShareId,
        epoch: ShareEpoch,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemoteShareState {
    #[default]
    Inactive,
    Active {
        share_id: ShareId,
        epoch: ShareEpoch,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSessionSnapshot {
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub phase: PeerSessionPhase,
    pub local_share: LocalShareState,
    pub remote_share: RemoteShareState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerSessionServiceSnapshot {
    pub sessions: Arc<Vec<PeerSessionSnapshot>>,
}

impl PeerSessionServiceSnapshot {
    pub fn session(&self, session_id: SessionId) -> Option<&PeerSessionSnapshot> {
        self.sessions.iter().find(|session| session.session_id == session_id)
    }

    pub fn session_for_peer(&self, peer_id: &PeerId) -> Option<&PeerSessionSnapshot> {
        self.sessions.iter().find(|session| &session.peer_id == peer_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCloseReason {
    LocalDisconnect,
    RemoteDisconnect,
    Rejected { reason: String },
    Cancelled,
    SignalingLost,
    ConnectionFailed { reason: String },
    ProtocolViolation { reason: String },
    TrustRevoked,
    ServiceShutdown,
}

/// Semantic events emitted after peer and session authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerSessionEvent {
    IncomingRequest {
        session_id: SessionId,
        peer_id: PeerId,
    },
    Connected {
        session_id: SessionId,
        peer_id: PeerId,
    },
    Closed {
        session_id: SessionId,
        peer_id: PeerId,
        reason: SessionCloseReason,
    },
    MessageSent {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    },
    MessageReceived {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    },
    MessageReceiptReceived {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    },
    LocalShareChanged {
        session_id: SessionId,
        peer_id: PeerId,
        state: LocalShareState,
    },
    RemoteShareChanged {
        session_id: SessionId,
        peer_id: PeerId,
        state: RemoteShareState,
    },
}

impl PeerSessionEvent {
    pub fn session_id(&self) -> SessionId {
        match self {
            Self::IncomingRequest { session_id, .. }
            | Self::Connected { session_id, .. }
            | Self::Closed { session_id, .. }
            | Self::MessageSent { session_id, .. }
            | Self::MessageReceived { session_id, .. }
            | Self::MessageReceiptReceived { session_id, .. }
            | Self::LocalShareChanged { session_id, .. }
            | Self::RemoteShareChanged { session_id, .. } => *session_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_ids_are_available_through_the_peer_session_api() {
        assert_eq!(PeerId::new("peer-a").unwrap().as_str(), "peer-a");
        assert!(PeerId::new(" peer-a ").is_err());
    }

    #[test]
    fn snapshot_lookups_use_typed_identifiers() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer-a").unwrap();
        let snapshot = PeerSessionServiceSnapshot {
            sessions: Arc::new(vec![PeerSessionSnapshot {
                session_id,
                peer_id: peer_id.clone(),
                phase: PeerSessionPhase::Connected,
                local_share: LocalShareState::Inactive,
                remote_share: RemoteShareState::Inactive,
            }]),
        };

        assert_eq!(snapshot.session(session_id).unwrap().peer_id, peer_id);
        assert_eq!(snapshot.session_for_peer(&peer_id).unwrap().session_id, session_id);
    }

    #[test]
    fn share_epoch_is_nonzero_and_never_wraps() {
        assert!(ShareEpoch::try_from(0).is_err());
        assert_eq!(ShareEpoch::try_from(1).unwrap(), ShareEpoch::FIRST);
        assert!(ShareEpoch::try_from(u64::MAX).unwrap().next().is_err());
    }
}
