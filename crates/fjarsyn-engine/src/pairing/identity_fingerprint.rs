use std::fmt;

use sha2::{Digest, Sha256};

use crate::identity::PeerId;

const FINGERPRINT_DOMAIN: &[u8] = b"fjarsyn-peer-identity-v1\0";

/// Domain-separated SHA-256 digest of a peer ID and its Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdentityFingerprint([u8; 32]);

impl IdentityFingerprint {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(super) fn from_identity(peer_id: &PeerId, public_key: &[u8; 32]) -> Self {
        let peer_id = peer_id.as_str().as_bytes();
        let peer_id_length = u32::try_from(peer_id.len())
            .expect("validated peer IDs always fit in a u32")
            .to_be_bytes();
        let digest = Sha256::new()
            .chain_update(FINGERPRINT_DOMAIN)
            .chain_update(peer_id_length)
            .chain_update(peer_id)
            .chain_update(public_key)
            .finalize();

        Self(digest.into())
    }
}

impl fmt::Debug for IdentityFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IdentityFingerprint({self})")
    }
}

impl fmt::Display for IdentityFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, pair) in self.0.chunks_exact(2).enumerate() {
            if index != 0 {
                formatter.write_str(" ")?;
            }
            write!(formatter, "{:02X}{:02X}", pair[0], pair[1])?;
        }
        Ok(())
    }
}
