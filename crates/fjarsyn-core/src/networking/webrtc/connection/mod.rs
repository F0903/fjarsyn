use std::sync::{Arc, RwLock as StdRwLock};

use bytes::Bytes;
use tokio::sync::{RwLock, mpsc};
use webrtc::{
    api::media_engine::MIME_TYPE_H264, data_channel::RTCDataChannel,
    peer_connection::RTCPeerConnection, rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

use crate::networking::{
    protocol::SignalingMessage,
    signaling::{
        self, SignalingAuthContext,
        auth::{
            LocalPeerIdentity, StoredIdentityKeypair, TrustedPeerDirectory, TrustedPeerIdentity,
        },
    },
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

mod control;
mod handlers;
mod media;
mod messaging;
mod peer_connection;
mod signaling_flow;

#[derive(Debug, Clone)]
pub enum WebRTCEvent {
    Connected,
    Disconnected,
    IncomingCall(String),
    RemoteStreamStarted,
    RemoteStreamEnded,
}

#[derive(Debug, Clone)]
pub enum MessagingSignalEvent {
    IncomingMessage { from: String, payload: crate::networking::protocol::ChatMessagePayload },
    Receipt { from: String, payload: crate::networking::protocol::ChatReceiptPayload },
}

#[derive(Clone)]
pub struct WebRTC {
    pub peer_connection: Arc<RwLock<Arc<RTCPeerConnection>>>,
    pub signaling_tx: Arc<RwLock<Option<mpsc::Sender<SignalingMessage>>>>,
    pub base_signaling_tx: mpsc::Sender<SignalingMessage>,
    pub internal_signal_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<RwLock<Arc<TrackLocalStaticSample>>>,
    pub control_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>>,
    pub remote_peer_id: Arc<RwLock<Option<String>>>,
    pub message_signal_tx: Arc<RwLock<Option<mpsc::Sender<MessagingSignalEvent>>>>,
    pub local_peer_id: String,
    pub direct_signaling_port: u16,
    local_identity: LocalPeerIdentity,
    trusted_peers: Arc<StdRwLock<TrustedPeerDirectory>>,
    signaling_auth: SignalingAuthContext,
    packet_sink: mpsc::Sender<Bytes>,
    event_tx: mpsc::Sender<WebRTCEvent>,
    max_depacket_latency: u16,
    tasks: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl std::fmt::Debug for WebRTC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRTC")
            .field("local_peer_id", &self.local_peer_id)
            .field("direct_signaling_port", &self.direct_signaling_port)
            .field("max_depacket_latency", &self.max_depacket_latency)
            .finish_non_exhaustive()
    }
}

impl WebRTC {
    const STREAM_ID: &str = "fjarsyn-webrtc";

    pub async fn init(
        packet_sink: mpsc::Sender<Bytes>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        max_depacket_latency: u16,
        peer_id: Option<String>,
        identity_keypair: Option<StoredIdentityKeypair>,
    ) -> WebRTCResult<Arc<Self>> {
        let (signal_tx, signal_rx) = mpsc::channel(100);
        let local_identity = match identity_keypair.as_ref() {
            Some(identity) => LocalPeerIdentity::from_stored(identity)?,
            None => LocalPeerIdentity::generate(),
        };
        let trusted_peers: Arc<StdRwLock<TrustedPeerDirectory>> =
            Arc::new(StdRwLock::new(TrustedPeerDirectory::default()));
        let signaling_auth =
            SignalingAuthContext::new(local_identity.clone(), trusted_peers.clone());

        let local_peer_id = match std::env::var("FJARSYN_PEER_ID") {
            Ok(val) if val == "random" => uuid::Uuid::new_v4().to_string(),
            Ok(val) => val,
            Err(_) => peer_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        };

        let (listener_tx, direct_port) =
            signaling::listen(0, signaling_auth.clone(), local_peer_id.clone(), signal_tx.clone())
                .await?;
        let signaling_tx = Arc::new(RwLock::new(Some(listener_tx.clone())));

        let peer_connection = Arc::new(Self::create_pc().await?);
        let peer_connection_lock = Arc::new(RwLock::new(peer_connection.clone()));

        let video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability { mime_type: MIME_TYPE_H264.to_owned(), ..Default::default() },
            "video".to_owned(),
            Self::STREAM_ID.to_owned(),
        ));

        peer_connection
            .add_track(Arc::clone(&video_track)
                as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        let video_track_lock = Arc::new(RwLock::new(video_track));
        let control_channel = Arc::new(RwLock::new(None));
        let remote_peer_id = Arc::new(RwLock::new(None));
        let message_signal_tx = Arc::new(RwLock::new(None));

        let webrtc = Arc::new(Self {
            peer_connection: peer_connection_lock,
            signaling_tx,
            base_signaling_tx: listener_tx,
            internal_signal_tx: signal_tx,
            video_track: video_track_lock,
            control_channel,
            remote_peer_id,
            message_signal_tx,
            local_peer_id,
            direct_signaling_port: direct_port,
            local_identity,
            trusted_peers,
            signaling_auth,
            packet_sink,
            event_tx,
            max_depacket_latency,
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        });

        webrtc.clone().spawn_signaling_reader(signal_rx);
        webrtc.setup_pc_handlers(&peer_connection).await;

        Ok(webrtc)
    }

    pub fn local_public_key(&self) -> String {
        self.local_identity.public_key_base64()
    }

    pub fn replace_trusted_peers(&self, peers: impl IntoIterator<Item = TrustedPeerIdentity>) {
        let mut trusted_peers = self.trusted_peers.write().unwrap();
        *trusted_peers = TrustedPeerDirectory::new(peers);
    }

    pub(crate) fn signaling_auth_context(&self) -> SignalingAuthContext {
        self.signaling_auth.clone()
    }
}

impl Drop for WebRTC {
    fn drop(&mut self) {
        tracing::debug!("Aborting WebRTC tasks");
        let tasks = self.tasks.lock().unwrap();
        for task in tasks.iter() {
            task.abort();
        }
    }
}
