use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::{Error, PeerId, key_encoding::decode_verifying_key};

/// Public identity pinned to a contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeerIdentity {
    pub peer_id: PeerId,
    pub public_key: String,
}

impl TrustedPeerIdentity {
    pub fn new(peer_id: PeerId, public_key: impl Into<String>) -> Self {
        Self { peer_id, public_key: public_key.into() }
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.verifying_key().map(drop)
    }

    pub(crate) fn verifying_key(&self) -> Result<VerifyingKey, Error> {
        decode_verifying_key(&self.public_key)
    }
}
