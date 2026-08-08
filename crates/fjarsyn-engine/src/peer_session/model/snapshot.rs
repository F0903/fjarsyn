use std::sync::Arc;

use super::{LocalShareState, Phase, RemoteShareState, SessionId};
use crate::identity::PeerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub phase: Phase,
    pub local_share: LocalShareState,
    pub remote_share: RemoteShareState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub sessions: Arc<Vec<SessionSnapshot>>,
}

impl Snapshot {
    pub fn session(&self, session_id: SessionId) -> Option<&SessionSnapshot> {
        self.sessions.iter().find(|session| session.session_id == session_id)
    }

    pub fn session_for_peer(&self, peer_id: &PeerId) -> Option<&SessionSnapshot> {
        self.sessions.iter().find(|session| &session.peer_id == peer_id)
    }
}
