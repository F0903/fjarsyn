use std::{net::SocketAddr, sync::Arc, time::Instant};

use crate::identity::PeerId;

/// One mDNS advertisement contributing reachability hints for a nearby peer.
///
/// An advertisement is not proof of identity. Its endpoints must only be used to
/// bootstrap an authenticated connection to an already trusted peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyAdvertisement {
    pub instance_name: String,
    pub hostname: String,
    pub endpoints: Arc<[SocketAddr]>,
    pub last_seen: Instant,
}

/// The aggregate presence of one peer across all of its current mDNS
/// advertisements.
///
/// `hostname` and `instance_name` identify the most recently resolved
/// advertisement. `endpoints` is the de-duplicated union of every current
/// advertisement for this peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyPeer {
    /// Syntactically valid but unauthenticated peer ID claimed over mDNS.
    pub peer_id: PeerId,
    pub hostname: String,
    pub instance_name: String,
    pub endpoints: Arc<[SocketAddr]>,
    pub last_seen: Instant,
    pub advertisements: Arc<[NearbyAdvertisement]>,
}

impl NearbyPeer {
    /// Returns all current reachability hints for this peer.
    ///
    /// These addresses are unauthenticated mDNS data. Successful connection
    /// authentication, never endpoint selection, establishes the remote peer's
    /// identity.
    pub fn endpoint_hints(&self) -> Arc<[SocketAddr]> {
        self.endpoints.clone()
    }
}
