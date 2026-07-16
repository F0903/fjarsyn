use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use fjarsyn_core::{
    peer_session::{PeerEndpointResolver, PeerId, PeerSessionError},
    presence::PresenceHandle,
};

/// Breaks the startup ordering cycle cleanly: the session listener must bind
/// before its port can be advertised, while outgoing sessions resolve only
/// after presence has started.
#[derive(Debug, Default)]
pub(crate) struct DeferredEndpointResolver {
    presence: RwLock<Option<PresenceHandle>>,
}

impl DeferredEndpointResolver {
    pub fn install(&self, presence: PresenceHandle) {
        *self.presence.write().expect("endpoint resolver lock poisoned") = Some(presence);
    }
}

#[async_trait]
impl PeerEndpointResolver for DeferredEndpointResolver {
    async fn endpoint_hints_for(
        &self,
        peer_id: &PeerId,
    ) -> Result<Arc<[SocketAddr]>, PeerSessionError> {
        Ok(self
            .presence
            .read()
            .expect("endpoint resolver lock poisoned")
            .as_ref()
            .map(|presence| presence.endpoint_hints(peer_id.as_str()))
            .unwrap_or_else(|| Arc::from([])))
    }
}
