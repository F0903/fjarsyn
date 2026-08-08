use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{NegotiationSignal, SIGNALING_VERSION, SessionReplayCache};
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{Error, SessionId},
};

pub(in crate::peer_session) struct EnvelopeVerification<'a> {
    pub trusted_peer: &'a TrustedPeerIdentity,
    pub expected_local: &'a PeerId,
    pub expected_remote: Option<&'a PeerId>,
    pub expected_session: Option<SessionId>,
    pub now: DateTime<Utc>,
    pub max_age: Duration,
    pub max_clock_skew: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::peer_session) struct SignedSessionEnvelope {
    version: u8,
    message_id: Uuid,
    session_id: SessionId,
    from: PeerId,
    to: PeerId,
    created_at: DateTime<Utc>,
    pub(super) payload: NegotiationSignal,
    signature: String,
}

impl SignedSessionEnvelope {
    pub(in crate::peer_session) fn sign(
        identity: &LocalPeerIdentity,
        session_id: SessionId,
        from: PeerId,
        to: PeerId,
        payload: NegotiationSignal,
        now: DateTime<Utc>,
    ) -> Result<Self, Error> {
        let mut envelope = Self {
            version: SIGNALING_VERSION,
            message_id: Uuid::new_v4(),
            session_id,
            from,
            to,
            created_at: now,
            payload,
            signature: String::new(),
        };
        envelope.signature = BASE64.encode(identity.sign(&envelope.signing_bytes()?).to_bytes());
        Ok(envelope)
    }

    pub(in crate::peer_session) fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(in crate::peer_session) fn from(&self) -> &PeerId {
        &self.from
    }

    pub(in crate::peer_session) fn payload(&self) -> &NegotiationSignal {
        &self.payload
    }

    pub(in crate::peer_session) fn into_payload(self) -> NegotiationSignal {
        self.payload
    }

    pub(in crate::peer_session) fn verify(
        &self,
        verification: EnvelopeVerification<'_>,
        replay: &mut SessionReplayCache,
    ) -> Result<(), Error> {
        if self.version != SIGNALING_VERSION {
            return Err(Error::Protocol(format!("unsupported signaling version {}", self.version)));
        }
        if &self.to != verification.expected_local {
            return Err(Error::Protocol("signaling target mismatch".into()));
        }
        if let Some(expected_remote) = verification.expected_remote
            && &self.from != expected_remote
        {
            return Err(Error::Protocol("signaling sender mismatch".into()));
        }
        if let Some(expected_session) = verification.expected_session
            && self.session_id != expected_session
        {
            return Err(Error::Protocol("signaling session mismatch".into()));
        }
        if verification.trusted_peer.peer_id != self.from {
            return Err(Error::Protocol("trusted peer identity mismatch".into()));
        }
        if self.created_at < verification.now - verification.max_age {
            return Err(Error::Protocol("stale signaling message".into()));
        }
        if self.created_at > verification.now + verification.max_clock_skew {
            return Err(Error::Protocol("future signaling message".into()));
        }

        let signature = BASE64
            .decode(&self.signature)
            .map_err(|error| Error::Protocol(format!("invalid signature base64: {error}")))?;
        let signature = Signature::from_slice(&signature)
            .map_err(|_| Error::Protocol("invalid signature length".into()))?;
        verification
            .trusted_peer
            .verifying_key()
            .map_err(|error| Error::Protocol(error.to_string()))?
            .verify_strict(&self.signing_bytes()?, &signature)
            .map_err(|_| Error::Protocol("invalid signaling signature".into()))?;

        replay.remember(
            self.message_id,
            self.created_at,
            verification.now,
            verification.max_age,
        )?;
        Ok(())
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, Error> {
        #[derive(Serialize)]
        struct Unsigned<'a> {
            version: u8,
            message_id: Uuid,
            session_id: SessionId,
            from: &'a PeerId,
            to: &'a PeerId,
            created_at: DateTime<Utc>,
            payload: &'a NegotiationSignal,
        }

        serde_json::to_vec(&Unsigned {
            version: self.version,
            message_id: self.message_id,
            session_id: self.session_id,
            from: &self.from,
            to: &self.to,
            created_at: self.created_at,
            payload: &self.payload,
        })
        .map_err(|error| Error::Protocol(error.to_string()))
    }
}
