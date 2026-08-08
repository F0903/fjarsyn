use super::IdentityFingerprint;
use crate::identity::{PeerId, TrustedPeerIdentity};

/// A trusted identity produced only after canonical parsing, key validation,
/// fingerprint presentation, and an explicit confirmation action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPeerIdentity {
    trusted_identity: TrustedPeerIdentity,
}

impl VerifiedPeerIdentity {
    pub(super) fn from_trusted_identity(trusted_identity: TrustedPeerIdentity) -> Self {
        Self { trusted_identity }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.trusted_identity.peer_id
    }

    pub fn public_key_base64(&self) -> &str {
        &self.trusted_identity.public_key
    }

    pub fn trusted_identity(&self) -> &TrustedPeerIdentity {
        &self.trusted_identity
    }

    pub fn fingerprint(&self) -> IdentityFingerprint {
        let public_key = self
            .trusted_identity
            .verifying_key()
            .expect("verified peer identity retains a validated public key")
            .to_bytes();
        IdentityFingerprint::from_identity(&self.trusted_identity.peer_id, &public_key)
    }

    pub fn into_trusted_identity(self) -> TrustedPeerIdentity {
        self.trusted_identity
    }
}
