use crate::{
    identity::PeerId,
    peer_session::{CloseReason, Event, SessionId, actor::ActorInstanceId},
};

#[derive(Debug)]
pub(in crate::peer_session) struct Update {
    pub instance_id: ActorInstanceId,
    pub event: Event,
}

#[derive(Debug)]
pub(in crate::peer_session) struct Terminal {
    pub instance_id: ActorInstanceId,
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub reason: CloseReason,
}

#[derive(Debug)]
pub(in crate::peer_session) enum TaskExit {
    Completed {
        instance_id: ActorInstanceId,
        session_id: SessionId,
        peer_id: PeerId,
    },
    Panicked {
        instance_id: ActorInstanceId,
        session_id: SessionId,
        peer_id: PeerId,
        reason: String,
    },
}

impl TaskExit {
    pub(in crate::peer_session) fn into_parts(
        self,
    ) -> (ActorInstanceId, SessionId, PeerId, Option<String>) {
        match self {
            Self::Completed { instance_id, session_id, peer_id } => {
                (instance_id, session_id, peer_id, None)
            }
            Self::Panicked { instance_id, session_id, peer_id, reason } => {
                (instance_id, session_id, peer_id, Some(reason))
            }
        }
    }
}
