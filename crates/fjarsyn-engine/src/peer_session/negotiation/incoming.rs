use super::Connection;
use crate::{
    identity::PeerId,
    peer_session::{SessionId, TransportGeneration},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::peer_session) enum Intent {
    NewSession,
    Restart { generation: TransportGeneration },
}

pub(in crate::peer_session) struct Incoming {
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub authenticated_public_key: String,
    pub intent: Intent,
    pub connection: Connection,
}

impl std::fmt::Debug for Incoming {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Incoming")
            .field("session_id", &self.session_id)
            .field("peer_id", &self.peer_id)
            .field("intent", &self.intent)
            .finish_non_exhaustive()
    }
}
