use std::fmt;

use super::{LocalPeerIdentity, PeerId};

/// One indivisible local peer identifier and its in-memory signing identity.
///
/// The signing key is deliberately opaque and has no public serialization API.
/// Persistent loading and protection remain the responsibility of the engine's
/// private identity store.
#[derive(Clone)]
pub struct LocalIdentity {
    peer_id: PeerId,
    signing_identity: LocalPeerIdentity,
}

impl LocalIdentity {
    /// Creates a fresh signing identity for an explicitly selected peer ID.
    ///
    /// This is primarily useful to pairing workflows and tests. The canonical
    /// [`crate::Engine`] startup path loads or creates its stable identity
    /// through private protected storage instead.
    pub fn generate(peer_id: PeerId) -> Self {
        Self { peer_id, signing_identity: LocalPeerIdentity::generate() }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn public_key_base64(&self) -> String {
        self.signing_identity.public_key_base64()
    }

    pub(in crate::identity) fn from_parts(
        peer_id: PeerId,
        signing_identity: LocalPeerIdentity,
    ) -> Self {
        Self { peer_id, signing_identity }
    }

    pub(crate) fn signing_identity(&self) -> &LocalPeerIdentity {
        &self.signing_identity
    }
}

impl fmt::Debug for LocalIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalIdentity")
            .field("peer_id", &self.peer_id)
            .field("public_key", &self.public_key_base64())
            .finish()
    }
}
