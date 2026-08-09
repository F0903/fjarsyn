use std::{fmt, str::FromStr};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
};

use super::{IdentityFingerprint, VerifiedPeerIdentity};
use crate::identity::{self, LocalIdentity, PeerId, PeerIdError, TrustedPeerIdentity};

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    InvalidIdentity(#[from] identity::Error),
}

const TOKEN_SCHEME: &str = "fjarsyn";
const TOKEN_KIND: &str = "pair";
const TOKEN_VERSION: &str = "v1";
const TOKEN_FIELD_COUNT: usize = 5;
const PUBLIC_KEY_BYTES: usize = 32;

/// Maximum accepted byte length, including permitted outer whitespace.
pub const MAX_INVITE_BYTES: usize = 512;

/// A syntactically canonical invite containing a valid, non-weak Ed25519 key.
#[derive(Clone, PartialEq, Eq)]
pub struct Invite {
    peer_id: PeerId,
    pub(super) public_key: [u8; PUBLIC_KEY_BYTES],
}

impl Invite {
    /// Builds an invite from the standard-base64 key representation used by
    /// persisted trusted identities.
    pub fn new(peer_id: PeerId, public_key_base64: impl Into<String>) -> Result<Self, Error> {
        let identity = TrustedPeerIdentity::new(peer_id.clone(), public_key_base64);
        let public_key = identity.verifying_key()?.to_bytes();
        Ok(Self { peer_id, public_key })
    }

    /// Builds the invite published by this application instance.
    pub fn from_local(identity: &LocalIdentity) -> Self {
        let signing_identity = identity.signing_identity();
        let public_key = signing_identity.verifying_key().to_bytes();
        debug_assert!(!signing_identity.verifying_key().is_weak());
        Self { peer_id: identity.peer_id().clone(), public_key }
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
        IdentityFingerprint::from_identity(&self.peer_id, &self.public_key)
    }

    /// Marks the validated invite as explicitly confirmed by the user.
    pub fn confirm(self) -> VerifiedPeerIdentity {
        VerifiedPeerIdentity::from_trusted_identity(self.trusted_identity())
    }
}

impl fmt::Debug for Invite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Invite")
            .field("peer_id", &self.peer_id)
            .field("public_key", &self.public_key_base64())
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

impl fmt::Display for Invite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{TOKEN_SCHEME}:{TOKEN_KIND}:{TOKEN_VERSION}:{}:{}",
            URL_SAFE_NO_PAD.encode(self.peer_id.as_str().as_bytes()),
            URL_SAFE_NO_PAD.encode(self.public_key),
        )
    }
}

impl FromStr for Invite {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.len() > MAX_INVITE_BYTES {
            return Err(Error::TooLong { max: MAX_INVITE_BYTES });
        }

        let token = input.trim();
        let fields = token.split(':').collect::<Vec<_>>();
        if fields.len() != TOKEN_FIELD_COUNT {
            return Err(Error::InvalidFieldCount {
                expected: TOKEN_FIELD_COUNT,
                actual: fields.len(),
            });
        }
        if fields[0] != TOKEN_SCHEME {
            return Err(Error::InvalidScheme(fields[0].to_owned()));
        }
        if fields[1] != TOKEN_KIND {
            return Err(Error::InvalidKind(fields[1].to_owned()));
        }
        if fields[2] != TOKEN_VERSION {
            return Err(Error::UnsupportedVersion(fields[2].to_owned()));
        }

        let peer_id_bytes = decode_canonical_base64url("peer ID", fields[3])?;
        let peer_id = String::from_utf8(peer_id_bytes)
            .map_err(Error::InvalidPeerIdUtf8)
            .and_then(|value| PeerId::new(value).map_err(Error::InvalidPeerId))?;

        let public_key = decode_canonical_base64url("public key", fields[4])?;
        let actual = public_key.len();
        let public_key: [u8; PUBLIC_KEY_BYTES] = public_key
            .try_into()
            .map_err(|_| Error::InvalidPublicKeyLength { expected: PUBLIC_KEY_BYTES, actual })?;

        // Reuse the global trusted-identity boundary so malformed curve points
        // and small-order Ed25519 keys cannot enter through pairing.
        Self::new(peer_id, BASE64.encode(public_key))
    }
}

fn decode_canonical_base64url(field: &'static str, encoded: &str) -> Result<Vec<u8>, Error> {
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| Error::InvalidBase64Url { field, source })?;
    if URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(Error::NonCanonicalBase64Url { field });
    }
    Ok(decoded)
}
