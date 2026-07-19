use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

use super::{
    MessageId, PeerId, PeerSessionError, SessionId, ShareEpoch, ShareId,
    restart::TransportGeneration,
};
use crate::identity::{LocalPeerIdentity, TrustedPeerIdentity};

pub(crate) const SIGNALING_VERSION: u8 = 1;
pub(crate) const DATA_PROTOCOL_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum NegotiationSignal {
    EndpointHello { challenge: Uuid },
    EndpointProof { challenge: Uuid },
    Request {},
    Restart { generation: TransportGeneration },
    RestartAck { generation: TransportGeneration },
    Accept {},
    Offer { generation: TransportGeneration, sdp: String },
    Answer { generation: TransportGeneration, sdp: String },
    IceCandidate { generation: TransportGeneration, candidate: RTCIceCandidateInit },
    Ready { generation: TransportGeneration },
    ReadyAck { generation: TransportGeneration },
    Reject { reason: String },
    Cancel {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum ControlMessage {
    ShareStarted { version: u8, share_id: ShareId, epoch: ShareEpoch },
    ShareStopped { version: u8, share_id: ShareId, epoch: ShareEpoch },
    Disconnect { version: u8 },
}

impl ControlMessage {
    pub(crate) fn validate(&self) -> Result<(), PeerSessionError> {
        let version = match self {
            Self::ShareStarted { version, .. }
            | Self::ShareStopped { version, .. }
            | Self::Disconnect { version } => *version,
        };
        validate_data_version(version)?;
        match self {
            Self::ShareStarted { epoch, .. } | Self::ShareStopped { epoch, .. } => {
                epoch.require_valid()
            }
            Self::Disconnect { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum MessagingMessage {
    Chat { version: u8, message_id: MessageId, body: String, sent_at: DateTime<Utc> },
    Receipt { version: u8, message_id: MessageId, received_at: DateTime<Utc> },
}

impl MessagingMessage {
    pub(crate) fn validate(&self, max_body_bytes: usize) -> Result<(), PeerSessionError> {
        let version = match self {
            Self::Chat { version, body, .. } => {
                if body.trim().is_empty() {
                    return Err(PeerSessionError::EmptyMessage);
                }
                if body.len() > max_body_bytes {
                    return Err(PeerSessionError::MessageTooLarge { max: max_body_bytes });
                }
                *version
            }
            Self::Receipt { version, .. } => *version,
        };
        validate_data_version(version)
    }
}

fn validate_data_version(version: u8) -> Result<(), PeerSessionError> {
    if version == DATA_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(PeerSessionError::Protocol(format!("unsupported data protocol version {version}")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedSessionEnvelope {
    version: u8,
    message_id: Uuid,
    session_id: SessionId,
    from: PeerId,
    to: PeerId,
    created_at: DateTime<Utc>,
    payload: NegotiationSignal,
    signature: String,
}

pub(crate) struct EnvelopeVerification<'a> {
    pub trusted_peer: &'a TrustedPeerIdentity,
    pub expected_local: &'a PeerId,
    pub expected_remote: Option<&'a PeerId>,
    pub expected_session: Option<SessionId>,
    pub now: DateTime<Utc>,
    pub max_age: Duration,
    pub max_clock_skew: Duration,
}

impl SignedSessionEnvelope {
    pub(crate) fn sign(
        identity: &LocalPeerIdentity,
        session_id: SessionId,
        from: PeerId,
        to: PeerId,
        payload: NegotiationSignal,
        now: DateTime<Utc>,
    ) -> Result<Self, PeerSessionError> {
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

    pub(crate) fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(crate) fn from(&self) -> &PeerId {
        &self.from
    }

    pub(crate) fn payload(&self) -> &NegotiationSignal {
        &self.payload
    }

    pub(crate) fn into_payload(self) -> NegotiationSignal {
        self.payload
    }

    pub(crate) fn verify(
        &self,
        verification: EnvelopeVerification<'_>,
        replay: &mut SessionReplayCache,
    ) -> Result<(), PeerSessionError> {
        if self.version != SIGNALING_VERSION {
            return Err(PeerSessionError::Protocol(format!(
                "unsupported signaling version {}",
                self.version
            )));
        }
        if &self.to != verification.expected_local {
            return Err(PeerSessionError::Protocol("signaling target mismatch".into()));
        }
        if let Some(expected_remote) = verification.expected_remote
            && &self.from != expected_remote
        {
            return Err(PeerSessionError::Protocol("signaling sender mismatch".into()));
        }
        if let Some(expected_session) = verification.expected_session
            && self.session_id != expected_session
        {
            return Err(PeerSessionError::Protocol("signaling session mismatch".into()));
        }
        if verification.trusted_peer.peer_id != self.from {
            return Err(PeerSessionError::Protocol("trusted peer identity mismatch".into()));
        }
        if self.created_at < verification.now - verification.max_age {
            return Err(PeerSessionError::Protocol("stale signaling message".into()));
        }
        if self.created_at > verification.now + verification.max_clock_skew {
            return Err(PeerSessionError::Protocol("future signaling message".into()));
        }

        let signature = BASE64.decode(&self.signature).map_err(|error| {
            PeerSessionError::Protocol(format!("invalid signature base64: {error}"))
        })?;
        let signature = Signature::from_slice(&signature)
            .map_err(|_| PeerSessionError::Protocol("invalid signature length".into()))?;
        verification
            .trusted_peer
            .verifying_key()
            .map_err(|error| PeerSessionError::Protocol(error.to_string()))?
            .verify_strict(&self.signing_bytes()?, &signature)
            .map_err(|_| PeerSessionError::Protocol("invalid signaling signature".into()))?;

        replay.remember(
            self.message_id,
            self.created_at,
            verification.now,
            verification.max_age,
        )?;
        Ok(())
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, PeerSessionError> {
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
        .map_err(|error| PeerSessionError::Protocol(error.to_string()))
    }
}

#[derive(Debug)]
pub(crate) struct SessionReplayCache {
    seen: HashMap<Uuid, DateTime<Utc>>,
    capacity: usize,
}

impl SessionReplayCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self { seen: HashMap::with_capacity(capacity.min(4096)), capacity: capacity.max(1) }
    }

    fn remember(
        &mut self,
        message_id: Uuid,
        created_at: DateTime<Utc>,
        now: DateTime<Utc>,
        max_age: Duration,
    ) -> Result<(), PeerSessionError> {
        let oldest_allowed = now - max_age;
        self.seen.retain(|_, timestamp| *timestamp >= oldest_allowed);
        if self.seen.contains_key(&message_id) {
            return Err(PeerSessionError::Protocol("signaling replay detected".into()));
        }
        if self.seen.len() >= self.capacity {
            let oldest = self
                .seen
                .iter()
                .min_by_key(|(_, timestamp)| **timestamp)
                .map(|(message_id, _)| *message_id);
            if let Some(oldest) = oldest {
                self.seen.remove(&oldest);
            }
        }
        self.seen.insert(message_id, created_at);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_binds_target_peer_and_session_and_rejects_replay() {
        let identity = LocalPeerIdentity::generate();
        let remote = PeerId::new("remote").unwrap();
        let local = PeerId::new("local").unwrap();
        let trusted = TrustedPeerIdentity::new(remote.clone(), identity.public_key_base64());
        let session = SessionId::new();
        let now = Utc::now();
        let envelope = SignedSessionEnvelope::sign(
            &identity,
            session,
            remote.clone(),
            local.clone(),
            NegotiationSignal::Request {},
            now,
        )
        .unwrap();
        let mut replay = SessionReplayCache::new(8);

        envelope
            .verify(
                EnvelopeVerification {
                    trusted_peer: &trusted,
                    expected_local: &local,
                    expected_remote: Some(&remote),
                    expected_session: Some(session),
                    now,
                    max_age: Duration::minutes(5),
                    max_clock_skew: Duration::seconds(30),
                },
                &mut replay,
            )
            .unwrap();
        assert!(
            envelope
                .verify(
                    EnvelopeVerification {
                        trusted_peer: &trusted,
                        expected_local: &local,
                        expected_remote: Some(&remote),
                        expected_session: Some(session),
                        now,
                        max_age: Duration::minutes(5),
                        max_clock_skew: Duration::seconds(30),
                    },
                    &mut replay,
                )
                .is_err()
        );
    }

    #[test]
    fn chat_validation_enforces_version_and_body_limit() {
        let message = MessagingMessage::Chat {
            version: DATA_PROTOCOL_VERSION,
            message_id: MessageId::new(),
            body: "hello".into(),
            sent_at: Utc::now(),
        };
        assert!(message.validate(5).is_ok());
        assert_eq!(message.validate(4), Err(PeerSessionError::MessageTooLarge { max: 4 }));
    }

    #[test]
    fn share_control_requires_v2_and_a_nonzero_epoch() {
        let message = ControlMessage::ShareStarted {
            version: DATA_PROTOCOL_VERSION,
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };
        assert!(message.validate().is_ok());

        let encoded = serde_json::to_vec(&message).unwrap();
        assert_eq!(serde_json::from_slice::<ControlMessage>(&encoded).unwrap(), message);

        let wrong_version = ControlMessage::ShareStarted {
            version: DATA_PROTOCOL_VERSION - 1,
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };
        assert!(wrong_version.validate().is_err());

        let zero_epoch = ControlMessage::ShareStopped {
            version: DATA_PROTOCOL_VERSION,
            share_id: ShareId::new(),
            epoch: ShareEpoch::from_value(0),
        };
        assert!(zero_epoch.validate().is_err());

        let mut missing_epoch = serde_json::to_value(message).unwrap();
        missing_epoch.as_object_mut().unwrap().remove("epoch");
        assert!(serde_json::from_value::<ControlMessage>(missing_epoch).is_err());
    }

    #[test]
    fn envelope_rejects_wrong_sender_target_session_and_mutated_sdp() {
        let identity = LocalPeerIdentity::generate();
        let remote = PeerId::new("remote").unwrap();
        let local = PeerId::new("local").unwrap();
        let trusted = TrustedPeerIdentity::new(remote.clone(), identity.public_key_base64());
        let session = SessionId::new();
        let now = Utc::now();
        let envelope = SignedSessionEnvelope::sign(
            &identity,
            session,
            remote.clone(),
            local.clone(),
            NegotiationSignal::Offer {
                generation: TransportGeneration::INITIAL,
                sdp: "signed-sdp".into(),
            },
            now,
        )
        .unwrap();

        let verify = |envelope: &SignedSessionEnvelope,
                      expected_remote: &PeerId,
                      expected_local: &PeerId,
                      expected_session: SessionId| {
            envelope.verify(
                EnvelopeVerification {
                    trusted_peer: &trusted,
                    expected_local,
                    expected_remote: Some(expected_remote),
                    expected_session: Some(expected_session),
                    now,
                    max_age: Duration::minutes(5),
                    max_clock_skew: Duration::seconds(30),
                },
                &mut SessionReplayCache::new(8),
            )
        };

        assert!(verify(&envelope, &PeerId::new("other").unwrap(), &local, session).is_err());
        assert!(verify(&envelope, &remote, &PeerId::new("other").unwrap(), session).is_err());
        assert!(verify(&envelope, &remote, &local, SessionId::new()).is_err());

        let mut mutated = envelope;
        mutated.payload = NegotiationSignal::Offer {
            generation: TransportGeneration::INITIAL,
            sdp: "mutated-sdp".into(),
        };
        assert!(verify(&mutated, &remote, &local, session).is_err());
    }

    #[test]
    fn envelope_signature_binds_transport_generation() {
        let identity = LocalPeerIdentity::generate();
        let remote = PeerId::new("remote").unwrap();
        let local = PeerId::new("local").unwrap();
        let trusted = TrustedPeerIdentity::new(remote.clone(), identity.public_key_base64());
        let session = SessionId::new();
        let now = Utc::now();
        let mut envelope = SignedSessionEnvelope::sign(
            &identity,
            session,
            remote.clone(),
            local.clone(),
            NegotiationSignal::Restart { generation: TransportGeneration::from_value(1) },
            now,
        )
        .unwrap();
        envelope.payload =
            NegotiationSignal::Restart { generation: TransportGeneration::from_value(2) };

        assert!(
            envelope
                .verify(
                    EnvelopeVerification {
                        trusted_peer: &trusted,
                        expected_local: &local,
                        expected_remote: Some(&remote),
                        expected_session: Some(session),
                        now,
                        max_age: Duration::minutes(5),
                        max_clock_skew: Duration::seconds(30),
                    },
                    &mut SessionReplayCache::new(8),
                )
                .is_err()
        );
    }

    #[test]
    fn wire_types_reject_unknown_fields_and_invalid_peer_ids() {
        assert!(serde_json::from_str::<PeerId>(r#""""#).is_err());
        assert!(
            serde_json::from_str::<NegotiationSignal>(r#"{"kind":"request","unexpected":true}"#,)
                .is_err()
        );

        let identity = LocalPeerIdentity::generate();
        let envelope = SignedSessionEnvelope::sign(
            &identity,
            SessionId::new(),
            PeerId::new("remote").unwrap(),
            PeerId::new("local").unwrap(),
            NegotiationSignal::Request {},
            Utc::now(),
        )
        .unwrap();
        let mut value = serde_json::to_value(envelope).unwrap();
        value.as_object_mut().unwrap().insert("unsigned-extra".into(), true.into());
        assert!(serde_json::from_value::<SignedSessionEnvelope>(value).is_err());
    }
}
