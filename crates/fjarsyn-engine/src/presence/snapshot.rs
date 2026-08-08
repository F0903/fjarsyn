use std::{net::SocketAddr, sync::Arc};

use super::NearbyPeer;
use crate::identity::PeerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    revision: u64,
    peers: Arc<[NearbyPeer]>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self { revision: 0, peers: Arc::from([]) }
    }
}

impl Snapshot {
    pub(super) fn new(revision: u64, peers: Arc<[NearbyPeer]>) -> Self {
        Self { revision, peers }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn peers(&self) -> &[NearbyPeer] {
        &self.peers
    }

    pub fn peer(&self, peer_id: &PeerId) -> Option<&NearbyPeer> {
        self.peers.iter().find(|peer| &peer.peer_id == peer_id)
    }

    pub fn is_nearby(&self, peer_id: &PeerId) -> bool {
        self.peer(peer_id).is_some()
    }

    /// Returns all current endpoint hints for a peer, or an empty slice when
    /// the peer is not nearby.
    ///
    /// Presence is deliberately unauthenticated. Callers must bind the
    /// resulting connection to the peer's trusted identity and must never
    /// treat selection of one of these addresses as identity proof.
    pub fn endpoint_hints(&self, peer_id: &PeerId) -> Arc<[SocketAddr]> {
        self.peer(peer_id).map(NearbyPeer::endpoint_hints).unwrap_or_else(|| Arc::from([]))
    }
}
