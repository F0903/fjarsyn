use std::{collections::HashMap, fmt};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::networking::protocol::SignalingMessage;

const PUBLIC_KEY_BYTES: usize = 32;
const PRIVATE_KEY_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;

pub const SIGNED_SIGNALING_VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SignalingAuthError {
    #[error("Invalid base64 {kind}: {source}")]
    InvalidBase64 { kind: &'static str, source: base64::DecodeError },
    #[error("Invalid {kind} length: expected {expected} bytes, got {actual}")]
    InvalidLength { kind: &'static str, expected: usize, actual: usize },
    #[error("Invalid signing key: stored public key does not match stored private key")]
    PublicKeyMismatch,
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(ed25519_dalek::SignatureError),
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Failed to serialize signed signaling payload: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Unsupported signed signaling version: {0}")]
    UnsupportedVersion(u8),
    #[error("Signed signaling sender mismatch: envelope={envelope}, payload={payload}")]
    SenderMismatch { envelope: String, payload: String },
    #[error("Signed signaling target mismatch")]
    TargetMismatch,
    #[error("Signed signaling peer mismatch: expected {expected}, got {actual}")]
    PeerMismatch { expected: String, actual: String },
    #[error("No trusted signaling key for peer {peer_id}")]
    NoTrustedPeer { peer_id: String },
    #[error("Signed signaling message is stale")]
    StaleMessage,
    #[error("Signed signaling message timestamp is too far in the future")]
    FutureMessage,
    #[error("Signed signaling replay detected for {from}/{message_id}")]
    ReplayDetected { from: String, message_id: String },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredIdentityKeypair {
    pub public_key: String,
    pub private_key: String,
}

impl fmt::Debug for StoredIdentityKeypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredIdentityKeypair")
            .field("public_key", &self.public_key)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct LocalPeerIdentity {
    signing_key: SigningKey,
}

impl fmt::Debug for LocalPeerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPeerIdentity").field("public_key", &self.public_key_base64()).finish()
    }
}

impl LocalPeerIdentity {
    pub fn generate() -> Self {
        Self { signing_key: SigningKey::generate(&mut OsRng) }
    }

    pub fn from_stored(stored: &StoredIdentityKeypair) -> Result<Self, SignalingAuthError> {
        let private_key = decode_array::<PRIVATE_KEY_BYTES>("private key", &stored.private_key)?;
        let public_key = decode_verifying_key(&stored.public_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);

        if signing_key.verifying_key() != public_key {
            return Err(SignalingAuthError::PublicKeyMismatch);
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

    fn sign(&self, bytes: &[u8]) -> Signature {
        self.signing_key.sign(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeerIdentity {
    pub peer_id: String,
    pub public_key: String,
}

impl TrustedPeerIdentity {
    pub fn new(peer_id: impl Into<String>, public_key: impl Into<String>) -> Self {
        Self { peer_id: peer_id.into(), public_key: public_key.into() }
    }

    pub fn validate(&self) -> Result<(), SignalingAuthError> {
        self.verifying_key().map(|_| ())
    }

    fn verifying_key(&self) -> Result<VerifyingKey, SignalingAuthError> {
        decode_verifying_key(&self.public_key)
    }
}

pub trait TrustedPeerStore {
    fn trusted_peer(&self, peer_id: &str) -> Option<TrustedPeerIdentity>;
}

#[derive(Debug, Clone, Default)]
pub struct TrustedPeerDirectory {
    peers: HashMap<String, TrustedPeerIdentity>,
}

impl TrustedPeerDirectory {
    pub fn new(peers: impl IntoIterator<Item = TrustedPeerIdentity>) -> Self {
        let peers =
            peers.into_iter().map(|peer| (peer.peer_id.clone(), peer)).collect::<HashMap<_, _>>();
        Self { peers }
    }

    pub fn insert(&mut self, peer: TrustedPeerIdentity) {
        self.peers.insert(peer.peer_id.clone(), peer);
    }
}

impl TrustedPeerStore for TrustedPeerDirectory {
    fn trusted_peer(&self, peer_id: &str) -> Option<TrustedPeerIdentity> {
        self.peers.get(peer_id).cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationOptions {
    pub max_message_age: Duration,
    pub max_clock_skew: Duration,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self { max_message_age: Duration::minutes(5), max_clock_skew: Duration::seconds(30) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedSignalingEnvelope {
    pub version: u8,
    pub message_id: String,
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub created_at: DateTime<Utc>,
    pub payload: SignalingMessage,
    pub signature: String,
}

impl SignedSignalingEnvelope {
    pub fn sign(
        identity: &LocalPeerIdentity,
        payload: SignalingMessage,
        now: DateTime<Utc>,
    ) -> Result<Self, SignalingAuthError> {
        let mut envelope = Self {
            version: SIGNED_SIGNALING_VERSION,
            message_id: uuid::Uuid::new_v4().to_string(),
            from: payload.from.clone(),
            to: payload.to.clone(),
            created_at: now,
            payload,
            signature: String::new(),
        };
        let signature = identity.sign(&envelope.signing_bytes()?);
        envelope.signature = BASE64.encode(signature.to_bytes());
        Ok(envelope)
    }

    pub fn verify(
        &self,
        trusted_peer: &TrustedPeerIdentity,
        replay_cache: &mut ReplayCache,
        now: DateTime<Utc>,
        options: VerificationOptions,
    ) -> Result<SignalingMessage, SignalingAuthError> {
        self.verify_without_replay(trusted_peer, now, options)?;
        replay_cache.remember(&self.from, &self.message_id, self.created_at, now, options)?;
        Ok(self.payload.clone())
    }

    pub fn verify_with_store(
        &self,
        trusted_peers: &impl TrustedPeerStore,
        replay_cache: &mut ReplayCache,
        now: DateTime<Utc>,
        options: VerificationOptions,
    ) -> Result<SignalingMessage, SignalingAuthError> {
        let trusted_peer = trusted_peers
            .trusted_peer(&self.from)
            .ok_or_else(|| SignalingAuthError::NoTrustedPeer { peer_id: self.from.clone() })?;
        self.verify(&trusted_peer, replay_cache, now, options)
    }

    pub fn verify_without_replay(
        &self,
        trusted_peer: &TrustedPeerIdentity,
        now: DateTime<Utc>,
        options: VerificationOptions,
    ) -> Result<(), SignalingAuthError> {
        self.validate_unsigned_fields(trusted_peer, now, options)?;

        let signature = decode_array::<SIGNATURE_BYTES>("signature", &self.signature)?;
        let signature = Signature::from_bytes(&signature);
        trusted_peer
            .verifying_key()?
            .verify(&self.signing_bytes()?, &signature)
            .map_err(|_| SignalingAuthError::InvalidSignature)
    }

    fn validate_unsigned_fields(
        &self,
        trusted_peer: &TrustedPeerIdentity,
        now: DateTime<Utc>,
        options: VerificationOptions,
    ) -> Result<(), SignalingAuthError> {
        if self.version != SIGNED_SIGNALING_VERSION {
            return Err(SignalingAuthError::UnsupportedVersion(self.version));
        }
        if self.from != self.payload.from {
            return Err(SignalingAuthError::SenderMismatch {
                envelope: self.from.clone(),
                payload: self.payload.from.clone(),
            });
        }
        if self.to != self.payload.to {
            return Err(SignalingAuthError::TargetMismatch);
        }
        if self.from != trusted_peer.peer_id {
            return Err(SignalingAuthError::PeerMismatch {
                expected: trusted_peer.peer_id.clone(),
                actual: self.from.clone(),
            });
        }
        if self.created_at < now - options.max_message_age {
            return Err(SignalingAuthError::StaleMessage);
        }
        if self.created_at > now + options.max_clock_skew {
            return Err(SignalingAuthError::FutureMessage);
        }

        Ok(())
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, SignalingAuthError> {
        let unsigned = UnsignedSignalingEnvelope {
            version: self.version,
            message_id: &self.message_id,
            from: &self.from,
            to: self.to.as_deref(),
            created_at: self.created_at,
            payload: &self.payload,
        };
        Ok(serde_json::to_vec(&unsigned)?)
    }
}

#[derive(Serialize)]
struct UnsignedSignalingEnvelope<'a> {
    version: u8,
    message_id: &'a str,
    from: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<&'a str>,
    created_at: DateTime<Utc>,
    payload: &'a SignalingMessage,
}

#[derive(Debug, Clone)]
pub struct ReplayCache {
    entries: HashMap<(String, String), DateTime<Utc>>,
    max_entries: usize,
}

impl ReplayCache {
    pub fn new(max_entries: usize) -> Self {
        Self { entries: HashMap::new(), max_entries: max_entries.max(1) }
    }

    pub fn remember(
        &mut self,
        from: &str,
        message_id: &str,
        created_at: DateTime<Utc>,
        now: DateTime<Utc>,
        options: VerificationOptions,
    ) -> Result<(), SignalingAuthError> {
        self.prune(now, options.max_message_age);

        let key = (from.to_string(), message_id.to_string());
        if self.entries.contains_key(&key) {
            return Err(SignalingAuthError::ReplayDetected {
                from: from.to_string(),
                message_id: message_id.to_string(),
            });
        }

        self.entries.insert(key, created_at);
        self.trim_to_capacity();
        Ok(())
    }

    fn prune(&mut self, now: DateTime<Utc>, max_message_age: Duration) {
        let oldest_allowed = now - max_message_age;
        self.entries.retain(|_, created_at| *created_at >= oldest_allowed);
    }

    fn trim_to_capacity(&mut self) {
        while self.entries.len() > self.max_entries {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, created_at)| **created_at)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.entries.remove(&oldest_key);
        }
    }
}

fn decode_verifying_key(public_key: &str) -> Result<VerifyingKey, SignalingAuthError> {
    let bytes = decode_array::<PUBLIC_KEY_BYTES>("public key", public_key)?;
    VerifyingKey::from_bytes(&bytes).map_err(SignalingAuthError::InvalidPublicKey)
}

fn decode_array<const N: usize>(
    kind: &'static str,
    value: &str,
) -> Result<[u8; N], SignalingAuthError> {
    let bytes = BASE64
        .decode(value)
        .map_err(|source| SignalingAuthError::InvalidBase64 { kind, source })?;
    let actual = bytes.len();
    bytes.try_into().map_err(|_| SignalingAuthError::InvalidLength { kind, expected: N, actual })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::protocol::SignalingType;

    fn message_from(peer_id: &str) -> SignalingMessage {
        SignalingMessage {
            from: peer_id.to_string(),
            to: Some("local-peer".into()),
            sig_type: SignalingType::Offer,
            data: "sdp".into(),
        }
    }

    #[test]
    fn generated_identity_round_trips_through_stored_form() {
        let identity = LocalPeerIdentity::generate();
        let stored = identity.to_stored();
        let restored = LocalPeerIdentity::from_stored(&stored).unwrap();

        assert_eq!(identity.public_key_base64(), restored.public_key_base64());
    }

    #[test]
    fn signed_envelope_verifies_with_trusted_peer_key() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let trusted_peer = TrustedPeerIdentity::new("peer-a", identity.public_key_base64());
        let envelope =
            SignedSignalingEnvelope::sign(&identity, message_from("peer-a"), now).unwrap();
        let mut replay_cache = ReplayCache::new(32);

        let payload = envelope
            .verify(&trusted_peer, &mut replay_cache, now, VerificationOptions::default())
            .unwrap();

        assert_eq!(payload, message_from("peer-a"));
    }

    #[test]
    fn trusted_peer_directory_resolves_by_peer_id() {
        let identity = LocalPeerIdentity::generate();
        let directory = TrustedPeerDirectory::new([TrustedPeerIdentity::new(
            "peer-a",
            identity.public_key_base64(),
        )]);

        assert!(directory.trusted_peer("peer-a").is_some());
        assert!(directory.trusted_peer("peer-b").is_none());
    }

    #[test]
    fn signed_envelope_verifies_with_trusted_peer_store() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let directory = TrustedPeerDirectory::new([TrustedPeerIdentity::new(
            "peer-a",
            identity.public_key_base64(),
        )]);
        let envelope =
            SignedSignalingEnvelope::sign(&identity, message_from("peer-a"), now).unwrap();
        let mut replay_cache = ReplayCache::new(32);

        let payload = envelope
            .verify_with_store(&directory, &mut replay_cache, now, VerificationOptions::default())
            .unwrap();

        assert_eq!(payload, message_from("peer-a"));
    }

    #[test]
    fn signed_envelope_rejects_wrong_trusted_key() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let other_identity = LocalPeerIdentity::generate();
        let trusted_peer = TrustedPeerIdentity::new("peer-a", other_identity.public_key_base64());
        let envelope =
            SignedSignalingEnvelope::sign(&identity, message_from("peer-a"), now).unwrap();

        let err = envelope
            .verify_without_replay(&trusted_peer, now, VerificationOptions::default())
            .unwrap_err();

        assert!(matches!(err, SignalingAuthError::InvalidSignature));
    }

    #[test]
    fn signed_envelope_rejects_sender_mismatch_before_signature_check() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let trusted_peer = TrustedPeerIdentity::new("peer-a", identity.public_key_base64());
        let mut envelope =
            SignedSignalingEnvelope::sign(&identity, message_from("peer-a"), now).unwrap();
        envelope.payload.from = "peer-b".into();

        let err = envelope
            .verify_without_replay(&trusted_peer, now, VerificationOptions::default())
            .unwrap_err();

        assert!(matches!(err, SignalingAuthError::SenderMismatch { .. }));
    }

    #[test]
    fn signed_envelope_rejects_stale_messages() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let trusted_peer = TrustedPeerIdentity::new("peer-a", identity.public_key_base64());
        let envelope = SignedSignalingEnvelope::sign(
            &identity,
            message_from("peer-a"),
            now - Duration::minutes(10),
        )
        .unwrap();

        let err = envelope
            .verify_without_replay(&trusted_peer, now, VerificationOptions::default())
            .unwrap_err();

        assert!(matches!(err, SignalingAuthError::StaleMessage));
    }

    #[test]
    fn replay_cache_rejects_reused_message_ids() {
        let now = Utc::now();
        let identity = LocalPeerIdentity::generate();
        let trusted_peer = TrustedPeerIdentity::new("peer-a", identity.public_key_base64());
        let envelope =
            SignedSignalingEnvelope::sign(&identity, message_from("peer-a"), now).unwrap();
        let mut replay_cache = ReplayCache::new(32);

        envelope
            .verify(&trusted_peer, &mut replay_cache, now, VerificationOptions::default())
            .unwrap();
        let err = envelope
            .verify(&trusted_peer, &mut replay_cache, now, VerificationOptions::default())
            .unwrap_err();

        assert!(matches!(err, SignalingAuthError::ReplayDetected { .. }));
    }
}
