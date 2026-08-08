use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;

use crate::{identity::PeerId, peer_session::Error};

#[async_trait]
pub trait EndpointResolver: Send + Sync {
    /// Returns one immutable, ordered snapshot of the peer's current endpoint
    /// hints. These addresses are unauthenticated discovery data; successful
    /// signaling authentication, never endpoint selection, establishes peer
    /// identity.
    async fn endpoint_hints_for(&self, peer_id: &PeerId) -> Result<Arc<[SocketAddr]>, Error>;
}
