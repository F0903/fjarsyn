use std::fmt;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use super::{
    Error,
    key_encoding::{PRIVATE_KEY_BYTES, decode_array, decode_verifying_key},
};

/// Serializable representation of the local signing identity.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredIdentityKeypair {
    pub public_key: String,
    pub private_key: String,
}

impl fmt::Debug for StoredIdentityKeypair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredIdentityKeypair")
            .field("public_key", &self.public_key)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

/// The local Ed25519 identity. Private key material is never exposed directly.
#[derive(Clone)]
pub struct LocalPeerIdentity {
    signing_key: SigningKey,
}

impl LocalPeerIdentity {
    pub fn generate() -> Self {
        loop {
            let signing_key = SigningKey::generate(&mut OsRng);
            if !signing_key.verifying_key().is_weak() {
                return Self { signing_key };
            }
        }
    }

    pub fn from_stored(stored: &StoredIdentityKeypair) -> Result<Self, Error> {
        let private_key = decode_array::<PRIVATE_KEY_BYTES>("private key", &stored.private_key)?;
        let public_key = decode_verifying_key(&stored.public_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);
        if signing_key.verifying_key() != public_key {
            return Err(Error::PublicKeyMismatch);
        }
        Ok(Self { signing_key })
    }

    pub fn to_stored(&self) -> StoredIdentityKeypair {
        StoredIdentityKeypair {
            public_key: self.public_key_base64(),
            private_key: BASE64.encode(self.signing_key.to_bytes()),
        }
    }

    pub fn public_key_base64(&self) -> String {
        BASE64.encode(self.signing_key.verifying_key().to_bytes())
    }

    pub(crate) fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub(crate) fn sign(&self, bytes: &[u8]) -> Signature {
        self.signing_key.sign(bytes)
    }
}

impl fmt::Debug for LocalPeerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalPeerIdentity")
            .field("public_key", &self.public_key_base64())
            .finish()
    }
}
