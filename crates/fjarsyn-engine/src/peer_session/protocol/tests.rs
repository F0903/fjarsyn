use chrono::{Duration, Utc};

use super::{
    ControlMessage, DATA_PROTOCOL_VERSION, EnvelopeVerification, MessagingMessage,
    NegotiationSignal, SessionReplayCache, SignedSessionEnvelope,
};
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{Error, MessageId, SessionId, ShareEpoch, ShareId, TransportGeneration},
};

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
    assert_eq!(message.validate(4), Err(Error::MessageTooLarge { max: 4 }));
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
