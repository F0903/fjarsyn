use std::{fmt, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use tokio::sync::watch;

use super::{NearbyPeer, NearbyPeers};
use crate::{identity::PeerId, peer_session};

/// Cloneable, read-only access to live presence snapshots.
///
/// The handle intentionally exposes no connection operation: mDNS only
/// provides unauthenticated reachability hints.
#[derive(Clone)]
pub struct ServiceHandle {
    snapshots: watch::Receiver<NearbyPeers>,
}

impl ServiceHandle {
    pub(super) fn new(snapshots: watch::Receiver<NearbyPeers>) -> Self {
        Self { snapshots }
    }

    pub fn snapshot(&self) -> NearbyPeers {
        self.snapshots.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<NearbyPeers> {
        self.snapshots.clone()
    }

    pub fn nearby_peer(&self, peer_id: &PeerId) -> Option<NearbyPeer> {
        self.snapshots.borrow().peer(peer_id).cloned()
    }

    /// Returns all current endpoint hints for `peer_id`.
    ///
    /// The returned addresses are unauthenticated mDNS data. Backend
    /// composition must verify the expected trusted identity while establishing
    /// a session and must never treat endpoint selection as identity proof.
    pub fn endpoint_hints(&self, peer_id: &PeerId) -> Arc<[SocketAddr]> {
        self.snapshots.borrow().endpoint_hints(peer_id)
    }
}

#[async_trait]
impl peer_session::EndpointResolver for ServiceHandle {
    async fn endpoint_hints_for(
        &self,
        peer_id: &PeerId,
    ) -> Result<Arc<[SocketAddr]>, peer_session::Error> {
        Ok(self.endpoint_hints(peer_id))
    }
}

impl fmt::Debug for ServiceHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceHandle")
            .field("snapshot", &*self.snapshots.borrow())
            .finish()
    }
}
