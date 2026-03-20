use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{RwLock, mpsc};
use webrtc::{
    api::media_engine::MIME_TYPE_H264, data_channel::RTCDataChannel,
    peer_connection::RTCPeerConnection, rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

use crate::networking::{
    protocol::SignalingMessage,
    signaling,
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

mod control;
mod handlers;
mod media;
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

#[derive(Clone)]
pub struct WebRTC {
    pub peer_connection: Arc<RwLock<Arc<RTCPeerConnection>>>,
    pub signaling_tx: Arc<RwLock<Option<mpsc::Sender<SignalingMessage>>>>,
    pub base_signaling_tx: mpsc::Sender<SignalingMessage>,
    pub internal_signal_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<RwLock<Arc<TrackLocalStaticSample>>>,
    pub control_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>>,
    pub remote_peer_id: Arc<RwLock<Option<String>>>,
    pub local_peer_id: String,
    pub direct_signaling_port: u16,
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
    ) -> WebRTCResult<Arc<Self>> {
        let (signal_tx, signal_rx) = mpsc::channel(100);

        let local_peer_id = match std::env::var("FJARSYN_PEER_ID") {
            Ok(val) if val == "random" => uuid::Uuid::new_v4().to_string(),
            Ok(val) => val,
            Err(_) => peer_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        };

        let (listener_tx, direct_port) = signaling::listen(0, signal_tx.clone()).await?;
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

        let webrtc = Arc::new(Self {
            peer_connection: peer_connection_lock,
            signaling_tx,
            base_signaling_tx: listener_tx,
            internal_signal_tx: signal_tx,
            video_track: video_track_lock,
            control_channel,
            remote_peer_id,
            local_peer_id,
            direct_signaling_port: direct_port,
            packet_sink,
            event_tx,
            max_depacket_latency,
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        });

        webrtc.clone().spawn_signaling_reader(signal_rx);
        webrtc.setup_pc_handlers(&peer_connection).await;

        Ok(webrtc)
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
