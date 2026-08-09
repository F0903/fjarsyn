use std::{fmt, sync::Arc};

use base64::{
    DecodeSliceError, Engine as _, decoded_len_estimate,
    engine::general_purpose::STANDARD as BASE64,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::{
    Error,
    key_encoding::{PRIVATE_KEY_BYTES, decode_verifying_key},
};

/// Serializable representation of the local signing identity.
#[derive(PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct StoredIdentityKeypair {
    pub(in crate::identity) public_key: String,
    pub(in crate::identity) private_key: String,
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
pub(crate) struct LocalPeerIdentity {
    signing_key: Arc<SigningKey>,
}

impl LocalPeerIdentity {
    pub(crate) fn generate() -> Self {
        loop {
            let signing_key = SigningKey::generate(&mut OsRng);
            if !signing_key.verifying_key().is_weak() {
                return Self { signing_key: Arc::new(signing_key) };
            }
        }
    }

    pub(in crate::identity) fn from_stored(stored: &StoredIdentityKeypair) -> Result<Self, Error> {
        let public_key = decode_verifying_key(&stored.public_key)?;
        let mut private_key = [0; PRIVATE_KEY_BYTES];
        decode_private_key(&stored.private_key, &mut private_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);
        private_key.zeroize();
        if signing_key.verifying_key() != public_key {
            return Err(Error::PublicKeyMismatch);
        }
        Ok(Self { signing_key: Arc::new(signing_key) })
    }

    pub(in crate::identity) fn to_stored(&self) -> StoredIdentityKeypair {
        let mut private_key_bytes = self.signing_key.to_bytes();
        let private_key = BASE64.encode(private_key_bytes.as_slice());
        private_key_bytes.zeroize();
        StoredIdentityKeypair { public_key: self.public_key_base64(), private_key }
    }

    pub(crate) fn public_key_base64(&self) -> String {
        BASE64.encode(self.signing_key.verifying_key().to_bytes())
    }

    pub(crate) fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub(crate) fn sign(&self, bytes: &[u8]) -> Signature {
        self.signing_key.sign(bytes)
    }
}

fn decode_private_key(value: &str, bytes: &mut [u8; PRIVATE_KEY_BYTES]) -> Result<(), Error> {
    match BASE64.decode_slice(value, bytes) {
        Ok(actual) if actual == PRIVATE_KEY_BYTES => Ok(()),
        Ok(actual) => {
            bytes.zeroize();
            Err(Error::InvalidLength { kind: "private key", expected: PRIVATE_KEY_BYTES, actual })
        }
        Err(DecodeSliceError::DecodeError(source)) => {
            bytes.zeroize();
            Err(Error::InvalidBase64 { kind: "private key", source })
        }
        Err(DecodeSliceError::OutputSliceTooSmall) => {
            bytes.zeroize();
            Err(Error::InvalidLength {
                kind: "private key",
                expected: PRIVATE_KEY_BYTES,
                actual: decoded_len_estimate(value.len()),
            })
        }
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
