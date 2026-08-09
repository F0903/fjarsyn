use std::sync::Arc;

use super::{Connection, Limits, SessionConnectionContext};
use crate::{
    identity::{LocalPeerIdentity, PeerId},
    peer_session::{EndpointResolver, Error, NetworkScope, SessionId, TrustedPeerResolver},
};

/// Resolves fresh discovery hints and opens an authenticated, identity-pinned
/// signaling connection. Session actors use the same service for initial
/// negotiation and short-lived ICE recovery.
#[derive(Clone)]
pub(in crate::peer_session) struct Service {
    local_peer_id: PeerId,
    local_identity: LocalPeerIdentity,
    trusted_peers: Arc<dyn TrustedPeerResolver>,
    endpoints: Arc<dyn EndpointResolver>,
    limits: Limits,
    network_scope: NetworkScope,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Service")
            .field("local_peer_id", &self.local_peer_id)
            .field("max_endpoint_attempts", &self.limits.max_endpoint_attempts)
            .field("network_scope", &self.network_scope)
            .finish_non_exhaustive()
    }
}

impl Service {
    pub(in crate::peer_session) fn new(
        local_peer_id: PeerId,
        local_identity: LocalPeerIdentity,
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        endpoints: Arc<dyn EndpointResolver>,
        limits: Limits,
        network_scope: NetworkScope,
    ) -> Self {
        Self { local_peer_id, local_identity, trusted_peers, endpoints, limits, network_scope }
    }

    pub(in crate::peer_session) async fn connect(
        &self,
        session_id: SessionId,
        remote_peer_id: PeerId,
    ) -> Result<Connection, Error> {
        let trusted_peer = self
            .trusted_peers
            .trusted_peer(&remote_peer_id)
            .await?
            .ok_or_else(|| Error::PeerNotTrusted(remote_peer_id.clone()))?;
        let endpoint_hints = self.endpoints.endpoint_hints_for(&remote_peer_id).await?;
        if endpoint_hints.is_empty() {
            return Err(Error::PeerNotNearby(remote_peer_id));
        }
        Connection::connect_from_hints(
            &endpoint_hints,
            self.network_scope,
            SessionConnectionContext {
                session_id,
                local_peer_id: self.local_peer_id.clone(),
                remote_peer_id,
                local_identity: self.local_identity.clone(),
                trusted_peer,
                limits: self.limits.clone(),
            },
        )
        .await
    }
}
