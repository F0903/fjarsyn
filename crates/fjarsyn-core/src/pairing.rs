//! Explicit, human-confirmed exchange of long-lived peer identities.
//!
//! Pairing invites are transport-neutral text. Parsing an invite validates its
//! canonical representation and cryptographic key, while [`PairingInvite::confirm`]
//! records the separate semantic step in which a user confirmed the fingerprint.

use std::{fmt, str::FromStr};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
};
use sha2::{Digest, Sha256};

use crate::identity::{IdentityError, LocalPeerIdentity, PeerId, PeerIdError, TrustedPeerIdentity};

const TOKEN_SCHEME: &str = "fjarsyn";
const TOKEN_KIND: &str = "pair";
const TOKEN_VERSION: &str = "v1";
const TOKEN_FIELD_COUNT: usize = 5;
const PUBLIC_KEY_BYTES: usize = 32;
const FINGERPRINT_DOMAIN: &[u8] = b"fjarsyn-peer-identity-v1\0";

/// Maximum accepted byte length, including permitted outer whitespace.
pub const MAX_PAIRING_INVITE_BYTES: usize = 512;

/// Domain-separated SHA-256 digest of a peer ID and its Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdentityFingerprint([u8; 32]);

impl IdentityFingerprint {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
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

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("pairing invite exceeds the {max} byte limit")]
    TooLong { max: usize },
    #[error("pairing invite must contain exactly {expected} colon-separated fields, got {actual}")]
    InvalidFieldCount { expected: usize, actual: usize },
    #[error("unsupported pairing invite scheme {0:?}")]
    InvalidScheme(String),
    #[error("unsupported pairing invite kind {0:?}")]
    InvalidKind(String),
    #[error("unsupported pairing invite version {0:?}")]
    UnsupportedVersion(String),
    #[error("invalid base64url in pairing invite {field}: {source}")]
    InvalidBase64Url {
        field: &'static str,
        #[source]
        source: base64::DecodeError,
    },
    #[error("pairing invite {field} is not canonically base64url encoded")]
    NonCanonicalBase64Url { field: &'static str },
    #[error("pairing invite peer ID is not valid UTF-8")]
    InvalidPeerIdUtf8(#[source] std::string::FromUtf8Error),
    #[error("invalid pairing invite peer ID: {0}")]
    InvalidPeerId(#[from] PeerIdError),
    #[error("invalid pairing public key length: expected {expected} bytes, got {actual}")]
    InvalidPublicKeyLength { expected: usize, actual: usize },
    #[error("invalid pairing identity: {0}")]
    InvalidIdentity(#[from] IdentityError),
}

/// A syntactically canonical invite containing a valid, non-weak Ed25519 key.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingInvite {
    peer_id: PeerId,
    public_key: [u8; PUBLIC_KEY_BYTES],
}

impl PairingInvite {
    /// Builds an invite from the standard-base64 key representation used by
    /// persisted trusted identities.
    pub fn new(
        peer_id: PeerId,
        public_key_base64: impl Into<String>,
    ) -> Result<Self, PairingError> {
        let identity = TrustedPeerIdentity::new(peer_id.clone(), public_key_base64);
        let public_key = identity.verifying_key()?.to_bytes();
        Ok(Self { peer_id, public_key })
    }

    /// Builds the invite published by this application instance.
    pub fn from_local(peer_id: PeerId, identity: &LocalPeerIdentity) -> Self {
        let public_key = identity.verifying_key().to_bytes();
        debug_assert!(!identity.verifying_key().is_weak());
        Self { peer_id, public_key }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the canonical, padded standard-base64 representation used by
    /// [`TrustedPeerIdentity`].
    pub fn public_key_base64(&self) -> String {
        BASE64.encode(self.public_key)
    }

    pub fn trusted_identity(&self) -> TrustedPeerIdentity {
        TrustedPeerIdentity::new(self.peer_id.clone(), self.public_key_base64())
    }

    /// Full SHA-256 fingerprint grouped for human comparison. No bytes are
    /// truncated, so two users can compare it through any independent channel.
    pub fn fingerprint(&self) -> IdentityFingerprint {
        identity_fingerprint(&self.peer_id, &self.public_key)
    }

    /// Marks the validated invite as explicitly confirmed by the user.
    pub fn confirm(self) -> VerifiedPeerIdentity {
        VerifiedPeerIdentity { trusted_identity: self.trusted_identity() }
    }
}

impl fmt::Debug for PairingInvite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingInvite")
            .field("peer_id", &self.peer_id)
            .field("public_key", &self.public_key_base64())
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

impl fmt::Display for PairingInvite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{TOKEN_SCHEME}:{TOKEN_KIND}:{TOKEN_VERSION}:{}:{}",
            URL_SAFE_NO_PAD.encode(self.peer_id.as_str().as_bytes()),
            URL_SAFE_NO_PAD.encode(self.public_key),
        )
    }
}

impl FromStr for PairingInvite {
    type Err = PairingError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.len() > MAX_PAIRING_INVITE_BYTES {
            return Err(PairingError::TooLong { max: MAX_PAIRING_INVITE_BYTES });
        }

        let token = input.trim();
        let fields = token.split(':').collect::<Vec<_>>();
        if fields.len() != TOKEN_FIELD_COUNT {
            return Err(PairingError::InvalidFieldCount {
                expected: TOKEN_FIELD_COUNT,
                actual: fields.len(),
            });
        }
        if fields[0] != TOKEN_SCHEME {
            return Err(PairingError::InvalidScheme(fields[0].to_owned()));
        }
        if fields[1] != TOKEN_KIND {
            return Err(PairingError::InvalidKind(fields[1].to_owned()));
        }
        if fields[2] != TOKEN_VERSION {
            return Err(PairingError::UnsupportedVersion(fields[2].to_owned()));
        }

        let peer_id_bytes = decode_canonical_base64url("peer ID", fields[3])?;
        let peer_id = String::from_utf8(peer_id_bytes)
            .map_err(PairingError::InvalidPeerIdUtf8)
            .and_then(|value| PeerId::new(value).map_err(PairingError::InvalidPeerId))?;

        let public_key = decode_canonical_base64url("public key", fields[4])?;
        let actual = public_key.len();
        let public_key: [u8; PUBLIC_KEY_BYTES] = public_key.try_into().map_err(|_| {
            PairingError::InvalidPublicKeyLength { expected: PUBLIC_KEY_BYTES, actual }
        })?;

        // Reuse the global trusted-identity boundary so malformed curve points
        // and small-order Ed25519 keys cannot enter through pairing.
        Self::new(peer_id, BASE64.encode(public_key))
    }
}

/// A trusted identity produced only after canonical parsing, key validation,
/// fingerprint presentation, and an explicit confirmation action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPeerIdentity {
    trusted_identity: TrustedPeerIdentity,
}

impl VerifiedPeerIdentity {
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
        identity_fingerprint(&self.trusted_identity.peer_id, &public_key)
    }

    pub fn into_trusted_identity(self) -> TrustedPeerIdentity {
        self.trusted_identity
    }
}

fn decode_canonical_base64url(field: &'static str, encoded: &str) -> Result<Vec<u8>, PairingError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| PairingError::InvalidBase64Url { field, source })?;
    if URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(PairingError::NonCanonicalBase64Url { field });
    }
    Ok(decoded)
}

fn identity_fingerprint(
    peer_id: &PeerId,
    public_key: &[u8; PUBLIC_KEY_BYTES],
) -> IdentityFingerprint {
    let peer_id = peer_id.as_str().as_bytes();
    let peer_id_length =
        u32::try_from(peer_id.len()).expect("validated peer IDs always fit in a u32").to_be_bytes();
    let digest = Sha256::new()
        .chain_update(FINGERPRINT_DOMAIN)
        .chain_update(peer_id_length)
        .chain_update(peer_id)
        .chain_update(public_key)
        .finalize();

    IdentityFingerprint(digest.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_PEER_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
    const GOLDEN_PUBLIC_KEY_BASE64: &str = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";
    const GOLDEN_INVITE: &str = concat!(
        "fjarsyn:pair:v1:",
        "NTUwZTg0MDAtZTI5Yi00MWQ0LWE3MTYtNDQ2NjU1NDQwMDAw:",
        "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
    );
    const GOLDEN_FINGERPRINT: &str = concat!(
        "6DB5 E7B5 7B70 8C85 16D0 D38C CEA8 8341 ",
        "D7B3 7EFE 1653 945B 806E B54D C7C1 C2C8"
    );
    const GOLDEN_PUBLIC_KEY_BYTES: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];

    fn golden_invite() -> PairingInvite {
        PairingInvite::new(PeerId::new(GOLDEN_PEER_ID).unwrap(), GOLDEN_PUBLIC_KEY_BASE64).unwrap()
    }

    #[test]
    fn golden_invite_and_fingerprint_lock_the_wire_format() {
        let invite = golden_invite();

        assert_eq!(invite.to_string(), GOLDEN_INVITE);
        assert_eq!(invite.public_key, GOLDEN_PUBLIC_KEY_BYTES);
        assert_eq!(invite.fingerprint().to_string(), GOLDEN_FINGERPRINT);
        assert_eq!(GOLDEN_INVITE.parse::<PairingInvite>().unwrap(), invite);
    }

    #[test]
    fn parser_allows_only_outer_whitespace_and_display_is_canonical() {
        let parsed = format!(" \r\n{GOLDEN_INVITE}\t ").parse::<PairingInvite>().unwrap();

        assert_eq!(parsed.to_string(), GOLDEN_INVITE);
        let peer_with_space = URL_SAFE_NO_PAD.encode(b"peer-a ");
        let token = format!(
            "fjarsyn:pair:v1:{peer_with_space}:{}",
            URL_SAFE_NO_PAD.encode(parsed.public_key)
        );
        assert!(matches!(
            token.parse::<PairingInvite>(),
            Err(PairingError::InvalidPeerId(PeerIdError::InvalidCharacter {
                index: 6,
                character: ' '
            }))
        ));
    }

    #[test]
    fn parser_rejects_wrong_structure_and_version() {
        assert!(matches!(
            "".parse::<PairingInvite>(),
            Err(PairingError::InvalidFieldCount { actual: 1, .. })
        ));
        assert!(matches!(
            format!("{GOLDEN_INVITE}:extra").parse::<PairingInvite>(),
            Err(PairingError::InvalidFieldCount { actual: 6, .. })
        ));
        assert!(matches!(
            GOLDEN_INVITE.replacen("fjarsyn", "other", 1).parse::<PairingInvite>(),
            Err(PairingError::InvalidScheme(_))
        ));
        assert!(matches!(
            GOLDEN_INVITE.replacen(":pair:", ":other:", 1).parse::<PairingInvite>(),
            Err(PairingError::InvalidKind(_))
        ));
        assert!(matches!(
            GOLDEN_INVITE.replacen(":v1:", ":v2:", 1).parse::<PairingInvite>(),
            Err(PairingError::UnsupportedVersion(version)) if version == "v2"
        ));
    }

    #[test]
    fn parser_rejects_noncanonical_and_malformed_fields() {
        let padded = format!("{GOLDEN_INVITE}=");
        assert!(matches!(
            padded.parse::<PairingInvite>(),
            Err(PairingError::InvalidBase64Url { field: "public key", .. })
                | Err(PairingError::NonCanonicalBase64Url { field: "public key" })
        ));

        let invalid_utf8 =
            format!("fjarsyn:pair:v1:_w:{}", URL_SAFE_NO_PAD.encode(golden_invite().public_key));
        assert!(matches!(
            invalid_utf8.parse::<PairingInvite>(),
            Err(PairingError::InvalidPeerIdUtf8(_))
        ));

        let short_key = URL_SAFE_NO_PAD.encode([7_u8; PUBLIC_KEY_BYTES - 1]);
        let short = format!("fjarsyn:pair:v1:cGVlcg:{short_key}");
        assert!(matches!(
            short.parse::<PairingInvite>(),
            Err(PairingError::InvalidPublicKeyLength { actual: 31, .. })
        ));
    }

    #[test]
    fn parser_rejects_weak_ed25519_keys() {
        let mut identity_point = [0_u8; PUBLIC_KEY_BYTES];
        identity_point[0] = 1;
        let weak = format!("fjarsyn:pair:v1:cGVlcg:{}", URL_SAFE_NO_PAD.encode(identity_point));

        assert!(matches!(
            weak.parse::<PairingInvite>(),
            Err(PairingError::InvalidIdentity(IdentityError::WeakPublicKey))
        ));
    }

    #[test]
    fn parser_caps_the_complete_untrusted_input() {
        let oversized = format!("{}{}", " ".repeat(MAX_PAIRING_INVITE_BYTES), GOLDEN_INVITE);

        assert!(matches!(
            oversized.parse::<PairingInvite>(),
            Err(PairingError::TooLong { max: MAX_PAIRING_INVITE_BYTES })
        ));
    }

    #[test]
    fn any_identity_mutation_changes_the_full_fingerprint() {
        let invite = golden_invite();
        let changed_peer = PairingInvite::new(
            PeerId::new("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            GOLDEN_PUBLIC_KEY_BASE64,
        )
        .unwrap();
        let changed_key =
            PairingInvite::from_local(invite.peer_id().clone(), &LocalPeerIdentity::generate());

        assert_ne!(invite.fingerprint(), changed_peer.fingerprint());
        assert_ne!(invite.fingerprint(), changed_key.fingerprint());
    }

    #[test]
    fn confirmation_preserves_the_exact_validated_identity() {
        let invite = golden_invite();
        let expected_fingerprint = invite.fingerprint();
        let expected_identity = invite.trusted_identity();
        let verified = invite.confirm();

        assert_eq!(verified.peer_id(), &expected_identity.peer_id);
        assert_eq!(verified.public_key_base64(), expected_identity.public_key);
        assert_eq!(verified.fingerprint(), expected_fingerprint);
        assert_eq!(verified.trusted_identity(), &expected_identity);
        assert_eq!(verified.into_trusted_identity(), expected_identity);
    }

    #[test]
    fn local_invites_round_trip_through_the_text_format() {
        let peer_id = PeerId::new("local-peer_1.example").unwrap();
        let invite = PairingInvite::from_local(peer_id, &LocalPeerIdentity::generate());

        assert_eq!(invite.to_string().parse::<PairingInvite>().unwrap(), invite);
    }
}
