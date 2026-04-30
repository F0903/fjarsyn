use std::sync::{Arc, Mutex, RwLock};

use chrono::Utc;
use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use super::auth::{
    LocalPeerIdentity, ReplayCache, SignalingAuthError, SignedSignalingEnvelope,
    TrustedPeerDirectory, VerificationOptions,
};
use crate::networking::protocol::SignalingMessage;

#[derive(Debug, Clone)]
pub(crate) struct SignalingAuthContext {
    local_identity: LocalPeerIdentity,
    trusted_peers: Arc<RwLock<TrustedPeerDirectory>>,
    replay_cache: Arc<Mutex<ReplayCache>>,
    verification_options: VerificationOptions,
}

impl SignalingAuthContext {
    const REPLAY_CACHE_ENTRIES: usize = 4096;

    pub(crate) fn new(
        local_identity: LocalPeerIdentity,
        trusted_peers: Arc<RwLock<TrustedPeerDirectory>>,
    ) -> Self {
        Self {
            local_identity,
            trusted_peers,
            replay_cache: Arc::new(Mutex::new(ReplayCache::new(Self::REPLAY_CACHE_ENTRIES))),
            verification_options: VerificationOptions::default(),
        }
    }
}

pub(super) async fn send_signaling_message<S>(
    ws_stream: &mut WebSocketStream<S>,
    auth: &SignalingAuthContext,
    msg: &SignalingMessage,
) -> std::result::Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let envelope = SignedSignalingEnvelope::sign(&auth.local_identity, msg.clone(), Utc::now())
        .map_err(|err| {
            tracing::error!("Failed to sign signaling message: {}", err);
        })?;

    match serde_json::to_string(&envelope) {
        Ok(json) => {
            if ws_stream.send(Message::Text(json.into())).await.is_err() {
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            tracing::error!("Failed to serialize signaling message: {}", e);
            Err(())
        }
    }
}

pub(super) fn verify_incoming_signaling_message(
    auth: &SignalingAuthContext,
    text: &str,
) -> Result<SignalingMessage, SignalingAuthError> {
    let envelope = serde_json::from_str::<SignedSignalingEnvelope>(text)?;
    let trusted_peers = auth.trusted_peers.read().unwrap();
    let mut replay_cache = auth.replay_cache.lock().unwrap();

    envelope.verify_with_store(
        &*trusted_peers,
        &mut replay_cache,
        Utc::now(),
        auth.verification_options,
    )
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
    fn unsigned_signaling_message_is_rejected() {
        let local_identity = LocalPeerIdentity::generate();
        let trusted_peers = Arc::new(RwLock::new(TrustedPeerDirectory::default()));
        let auth = SignalingAuthContext::new(local_identity, trusted_peers);
        let unsigned = serde_json::to_string(&message_from("peer-a")).unwrap();

        assert!(verify_incoming_signaling_message(&auth, &unsigned).is_err());
    }

    #[test]
    fn signed_signaling_message_requires_trusted_peer() {
        let local_identity = LocalPeerIdentity::generate();
        let remote_identity = LocalPeerIdentity::generate();
        let trusted_peers = Arc::new(RwLock::new(TrustedPeerDirectory::default()));
        let auth = SignalingAuthContext::new(local_identity, trusted_peers);
        let envelope =
            SignedSignalingEnvelope::sign(&remote_identity, message_from("peer-a"), Utc::now())
                .unwrap();
        let signed = serde_json::to_string(&envelope).unwrap();

        assert!(matches!(
            verify_incoming_signaling_message(&auth, &signed),
            Err(SignalingAuthError::NoTrustedPeer { .. })
        ));
    }
}
