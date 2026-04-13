use std::sync::Arc;

use crate::{
    app::{
        AppLifecycle, AppState, ContactsState, MessagingState, NetworkingState, ServicesState,
        SessionState,
    },
    config::Config,
};

pub(crate) fn state() -> AppState {
    AppState {
        networking: NetworkingState {
            local_peer_id: None,
            discovered_peers: Vec::new(),
            recent_peers: Vec::new(),
        },
        session: SessionState {
            target_id: None,
            target_label: None,
            incoming_call_id: None,
            incoming_call_timeout: None,
            call_connected: false,
        },
        messaging: MessagingState {
            summaries: Arc::new(Vec::new()),
            active_peer_id: None,
            active_messages: Arc::new(Vec::new()),
            revision: 0,
        },
        contacts: ContactsState { contacts: Arc::new(Vec::new()) },
        config: Config::default(),
        services: ServicesState::default(),
        lifecycle: AppLifecycle::Bootstrapping,
    }
}
