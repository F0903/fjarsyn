//! Presence-backed endpoint resolution across startup ordering boundaries.

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;

use crate::{identity::PeerId, peer_session, presence};

/// Breaks the startup ordering cycle cleanly: the session listener must bind
/// before its port can be advertised, while outgoing sessions resolve only
/// after presence has started.
#[derive(Debug, Default)]
pub(crate) struct DeferredResolver {
    presence: OnceLock<presence::ServiceHandle>,
}

impl DeferredResolver {
    pub(crate) fn install(&self, presence: presence::ServiceHandle) {
        self.presence.set(presence).expect("presence handle is installed exactly once");
    }
}

#[async_trait]
impl peer_session::EndpointResolver for DeferredResolver {
    async fn endpoint_hints_for(
        &self,
        peer_id: &PeerId,
    ) -> Result<Arc<[SocketAddr]>, peer_session::Error> {
        Ok(self
            .presence
            .get()
            .map(|presence| presence.endpoint_hints(peer_id))
            .unwrap_or_else(|| Arc::from([])))
    }
}
