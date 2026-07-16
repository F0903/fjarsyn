//! Long-lived peer identities used to authenticate temporary session signaling.

use std::{fmt, str::FromStr};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

const PUBLIC_KEY_BYTES: usize = 32;
const PRIVATE_KEY_BYTES: usize = 32;
const MAX_PEER_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerIdError {
    #[error("peer ID cannot be empty")]
    Empty,
    #[error("peer ID exceeds the {max} byte limit")]
    TooLong { max: usize },
    #[error("peer ID must start with an ASCII letter or digit, got {character:?}")]
    InvalidStart { character: char },
    #[error(
        "peer ID contains invalid character {character:?} at byte {index}; only ASCII letters, digits, '.', '_', and '-' are allowed"
    )]
    InvalidCharacter { index: usize, character: char },
}

/// Stable identifier for a cryptographically trusted peer.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PeerId(String);

impl PeerId {
    pub fn new(value: impl Into<String>) -> Result<Self, PeerIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PeerIdError::Empty);
        }
        if value.len() > MAX_PEER_ID_BYTES {
            return Err(PeerIdError::TooLong { max: MAX_PEER_ID_BYTES });
        }

        let mut characters = value.char_indices();
        let (_, first) = characters.next().expect("non-empty peer ID has a first character");
        if !first.is_ascii_alphanumeric() {
            return Err(PeerIdError::InvalidStart { character: first });
        }
        if let Some((index, character)) =
            characters.find(|(_, character)| !is_peer_id_tail_character(*character))
        {
            return Err(PeerIdError::InvalidCharacter { index, character });
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_peer_id_tail_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

impl fmt::Debug for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PeerId").field(&self.0).finish()
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for PeerId {
    type Error = PeerIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for PeerId {
    type Error = PeerIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PeerId {
    type Err = PeerIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid base64 {kind}: {source}")]
    InvalidBase64 { kind: &'static str, source: base64::DecodeError },
    #[error("invalid {kind} length: expected {expected} bytes, got {actual}")]
    InvalidLength { kind: &'static str, expected: usize, actual: usize },
    #[error("stored public key does not match the stored private key")]
    PublicKeyMismatch,
    #[error("invalid public key: {0}")]
    InvalidPublicKey(ed25519_dalek::SignatureError),
    #[error("weak Ed25519 public keys are not accepted")]
    WeakPublicKey,
}

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

    pub fn from_stored(stored: &StoredIdentityKeypair) -> Result<Self, IdentityError> {
        let private_key = decode_array::<PRIVATE_KEY_BYTES>("private key", &stored.private_key)?;
        let public_key = decode_verifying_key(&stored.public_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);
        if signing_key.verifying_key() != public_key {
            return Err(IdentityError::PublicKeyMismatch);
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

    pub fn validate(&self) -> Result<(), IdentityError> {
        self.verifying_key().map(drop)
    }

    pub(crate) fn verifying_key(&self) -> Result<VerifyingKey, IdentityError> {
        decode_verifying_key(&self.public_key)
    }
}

fn decode_verifying_key(public_key: &str) -> Result<VerifyingKey, IdentityError> {
    let bytes = decode_array::<PUBLIC_KEY_BYTES>("public key", public_key)?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(IdentityError::InvalidPublicKey)?;
    if key.is_weak() {
        return Err(IdentityError::WeakPublicKey);
    }
    Ok(key)
}

fn decode_array<const N: usize>(kind: &'static str, value: &str) -> Result<[u8; N], IdentityError> {
    let bytes =
        BASE64.decode(value).map_err(|source| IdentityError::InvalidBase64 { kind, source })?;
    let actual = bytes.len();
    bytes.try_into().map_err(|_| IdentityError::InvalidLength { kind, expected: N, actual })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identity_round_trips_without_exposing_private_material_in_debug() {
        let identity = LocalPeerIdentity::generate();
        let stored = identity.to_stored();
        let restored = LocalPeerIdentity::from_stored(&stored).unwrap();

        assert_eq!(restored.public_key_base64(), identity.public_key_base64());
        assert!(!format!("{stored:?}").contains(&stored.private_key));
    }

    #[test]
    fn mismatched_stored_keypair_is_rejected() {
        let mut stored = LocalPeerIdentity::generate().to_stored();
        stored.public_key = LocalPeerIdentity::generate().public_key_base64();

        assert!(matches!(
            LocalPeerIdentity::from_stored(&stored),
            Err(IdentityError::PublicKeyMismatch)
        ));
    }

    #[test]
    fn trusted_identity_validates_key_encoding() {
        assert!(
            TrustedPeerIdentity::new(PeerId::new("peer-a").unwrap(), "not-a-key")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn peer_ids_enforce_the_canonical_ascii_grammar_without_trimming() {
        assert_eq!(PeerId::new("peer-A_1.example").unwrap().as_str(), "peer-A_1.example");
        assert_eq!(PeerId::new("").unwrap_err(), PeerIdError::Empty);
        assert_eq!(
            PeerId::new(" peer-a").unwrap_err(),
            PeerIdError::InvalidStart { character: ' ' }
        );
        assert_eq!(
            PeerId::new("peer-a ").unwrap_err(),
            PeerIdError::InvalidCharacter { index: 6, character: ' ' }
        );
        assert_eq!(
            PeerId::new("peer-\u{00e9}").unwrap_err(),
            PeerIdError::InvalidCharacter { index: 5, character: '\u{00e9}' }
        );
        assert_eq!(PeerId::new("-peer").unwrap_err(), PeerIdError::InvalidStart { character: '-' });
    }

    #[test]
    fn peer_id_length_limit_is_inclusive_and_measured_in_bytes() {
        assert!(PeerId::new("a".repeat(MAX_PEER_ID_BYTES)).is_ok());
        assert_eq!(
            PeerId::new("a".repeat(MAX_PEER_ID_BYTES + 1)).unwrap_err(),
            PeerIdError::TooLong { max: MAX_PEER_ID_BYTES }
        );
    }

    #[test]
    fn serde_and_from_str_use_the_same_strict_peer_id_boundary() {
        assert_eq!("peer-a".parse::<PeerId>().unwrap(), PeerId::new("peer-a").unwrap());
        assert!(" peer-a".parse::<PeerId>().is_err());
        assert!(serde_json::from_str::<PeerId>(r#""peer-a ""#).is_err());
    }

    #[test]
    fn trusted_identity_rejects_weak_ed25519_public_keys() {
        let mut identity_point = [0_u8; PUBLIC_KEY_BYTES];
        identity_point[0] = 1;
        let identity =
            TrustedPeerIdentity::new(PeerId::new("peer-a").unwrap(), BASE64.encode(identity_point));

        assert!(matches!(identity.validate(), Err(IdentityError::WeakPublicKey)));
    }
}
