use std::sync::Arc;

use super::AppLifecycle;
use crate::{
    communication::messaging::{ConversationMessage, ConversationSummary},
    config::Config,
    networking::discovery::PeerInfo,
    services::contacts_service::Contact,
};

pub struct NetworkingState {
    pub local_peer_id: Option<String>,
    pub discovered_peers: Vec<PeerInfo>,
    pub recent_peers: Vec<PeerInfo>,
}

pub struct SessionState {
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
    pub call_connected: bool,
}

pub struct MessagingState {
    pub summaries: Arc<Vec<ConversationSummary>>,
    pub active_peer_id: Option<String>,
    pub active_messages: Arc<Vec<ConversationMessage>>,
    pub revision: u64,
}

pub struct ContactsState {
    pub contacts: Arc<Vec<Contact>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePhase {
    Pending,
    Ready,
    Failed,
}

impl Default for ServicePhase {
    fn default() -> Self {
        Self::Pending
    }
}

impl ServicePhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Failed)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServicesState {
    pub database: ServicePhase,
    pub call: ServicePhase,
    pub discovery: ServicePhase,
    pub messaging: ServicePhase,
}

pub struct AppState {
    pub config: Config,
    pub lifecycle: AppLifecycle,
    pub services: ServicesState,
    pub networking: NetworkingState,
    pub session: SessionState,
    pub messaging: MessagingState,
    pub contacts: ContactsState,
}
