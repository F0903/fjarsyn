use crate::{
    identity::PeerId,
    peer_session::{CloseReason, Event, SessionId},
};

#[derive(Debug)]
pub(in crate::peer_session) struct Update {
    pub generation: uuid::Uuid,
    pub event: Event,
}

#[derive(Debug)]
pub(in crate::peer_session) struct Terminal {
    pub generation: uuid::Uuid,
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub reason: CloseReason,
}

#[derive(Debug)]
pub(in crate::peer_session) enum TaskExit {
    Completed { generation: uuid::Uuid, session_id: SessionId, peer_id: PeerId },
    Panicked { generation: uuid::Uuid, session_id: SessionId, peer_id: PeerId, reason: String },
}

impl TaskExit {
    pub(in crate::peer_session) fn into_parts(
        self,
    ) -> (uuid::Uuid, SessionId, PeerId, Option<String>) {
        match self {
            Self::Completed { generation, session_id, peer_id } => {
                (generation, session_id, peer_id, None)
            }
            Self::Panicked { generation, session_id, peer_id, reason } => {
                (generation, session_id, peer_id, Some(reason))
            }
        }
    }
}
