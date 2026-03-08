use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::{RwLock, mpsc};
use webrtc::{
    api::{
        APIBuilder,
        media_engine::{MIME_TYPE_H264, MediaEngine},
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    media::{Sample, io::sample_builder::SampleBuilder},
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        RTCRtpTransceiver,
        rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
    },
    track::{
        track_local::track_local_static_sample::TrackLocalStaticSample, track_remote::TrackRemote,
    },
};

use crate::networking::{
    protocol::{SignalingMessage, SignalingType},
    signaling,
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

#[derive(Debug, Clone)]
pub enum WebRTCEvent {
    Connected,
    Disconnected,
    IncomingCall(String),
}

/// Holds the state for the WebRTC connection manager
#[derive(Clone, Debug)]
pub struct WebRTC {
    pub peer_connection: Arc<RwLock<Arc<RTCPeerConnection>>>,
    pub signaling_tx: Arc<RwLock<Option<mpsc::Sender<SignalingMessage>>>>,
    pub internal_signal_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<RwLock<Arc<TrackLocalStaticSample>>>,
    pub remote_peer_id: Arc<RwLock<Option<String>>>,
    pub local_peer_id: String,
    pub direct_signaling_port: u16,

    // Persistent state to allow transparent re-initialization
    packet_sink: mpsc::Sender<Bytes>,
    event_tx: mpsc::Sender<WebRTCEvent>,
    max_depacket_latency: u16,
}

impl WebRTC {
    const STREAM_ID: &str = "fjarsyn-webrtc";

    pub async fn init(
        packet_sink: mpsc::Sender<Bytes>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        max_depacket_latency: u16,
    ) -> WebRTCResult<Self> {
        let (signal_tx, signal_rx) = mpsc::channel(100);
        let local_peer_id = uuid::Uuid::new_v4().to_string();

        let (listener_tx, direct_port) = signaling::listen(0, signal_tx.clone()).await?;
        let signaling_tx = Arc::new(RwLock::new(Some(listener_tx)));

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
        let remote_peer_id = Arc::new(RwLock::new(None));

        let webrtc = Self {
            peer_connection: peer_connection_lock,
            signaling_tx,
            internal_signal_tx: signal_tx,
            video_track: video_track_lock,
            remote_peer_id,
            local_peer_id,
            direct_signaling_port: direct_port,
            packet_sink,
            event_tx,
            max_depacket_latency,
        };

        // Setup Signaling Task
        webrtc.spawn_signaling_reader(signal_rx);

        // Setup Callbacks for the initial PC
        webrtc.setup_pc_handlers(&peer_connection).await;

        Ok(webrtc)
    }

    async fn create_pc() -> WebRTCResult<RTCPeerConnection> {
        let mut m = MediaEngine::default();
        m.register_default_codecs().map_err(WebRTCError::CodecError)?;
        let api = APIBuilder::new().with_media_engine(m).build();
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };
        api.new_peer_connection(config).await.map_err(WebRTCError::PeerConnectionError)
    }

    /// Internal method to ensure we have a fresh, usable PeerConnection.
    /// If the current one is closed, it replaces it.
    async fn prepare_pc(&self) -> WebRTCResult<Arc<RTCPeerConnection>> {
        let current_pc = self.peer_connection.read().await.clone();

        match current_pc.connection_state() {
            RTCPeerConnectionState::Closed | RTCPeerConnectionState::Failed => {
                tracing::info!(
                    "Current PC is {}, creating fresh connection...",
                    current_pc.connection_state()
                );

                let new_pc = Arc::new(Self::create_pc().await?);
                let new_video_track = Arc::new(TrackLocalStaticSample::new(
                    RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_H264.to_owned(),
                        ..Default::default()
                    },
                    "video".to_owned(),
                    Self::STREAM_ID.to_owned(),
                ));

                new_pc
                    .add_track(Arc::clone(&new_video_track)
                        as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>)
                    .await
                    .map_err(WebRTCError::PeerConnectionError)?;

                // Update shared state
                *self.peer_connection.write().await = new_pc.clone();
                *self.video_track.write().await = new_video_track;
                *self.remote_peer_id.write().await = None;

                // Setup handlers for the new PC
                self.setup_pc_handlers(&new_pc).await;

                Ok(new_pc)
            }
            _ => Ok(current_pc),
        }
    }

    async fn setup_pc_handlers(&self, pc: &Arc<RTCPeerConnection>) {
        Self::setup_ice_candidate_handler(
            pc,
            self.signaling_tx.clone(),
            self.local_peer_id.clone(),
        );
        Self::setup_connection_state_handler(pc, self.event_tx.clone());
        Self::setup_track_handler(pc, self.packet_sink.clone(), self.max_depacket_latency);
    }

    fn spawn_signaling_reader(&self, mut signal_rx: mpsc::Receiver<SignalingMessage>) {
        let this = self.clone();
        tokio::spawn(async move {
            while let Some(msg) = signal_rx.recv().await {
                if let Err(e) = this.handle_signaling_message(msg).await {
                    tracing::error!("Error handling signaling message: {}", e);
                }
            }
        });
    }

    async fn handle_signaling_message(&self, msg: SignalingMessage) -> WebRTCResult<()> {
        match msg.sig_type {
            SignalingType::Offer => {
                // IMPORTANT: Before applying an offer, ensure we have a fresh PC
                let pc = self.prepare_pc().await?;

                *self.remote_peer_id.write().await = Some(msg.from.clone());

                let sdp = RTCSessionDescription::offer(msg.data).map_err(WebRTCError::SdpError)?;
                pc.set_remote_description(sdp).await.map_err(WebRTCError::PeerConnectionError)?;

                let _ = self.event_tx.send(WebRTCEvent::IncomingCall(msg.from.clone())).await;
            }
            SignalingType::Answer => {
                let pc = self.peer_connection.read().await;
                let sdp = RTCSessionDescription::answer(msg.data).map_err(WebRTCError::SdpError)?;
                pc.set_remote_description(sdp).await.map_err(WebRTCError::PeerConnectionError)?;
            }
            SignalingType::Candidate => {
                let pc = self.peer_connection.read().await;
                let candidate: RTCIceCandidateInit =
                    serde_json::from_str(&msg.data).map_err(WebRTCError::DeserializeError)?;
                let _ = pc.add_ice_candidate(candidate).await;
            }
            SignalingType::Decline => {
                let pc = {
                    let lock = self.peer_connection.read().await;
                    lock.clone()
                };

                if pc.connection_state() == RTCPeerConnectionState::Closed {
                    return Ok(());
                }

                tracing::info!("Call declined by remote peer: {}", msg.from);
                pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
                *self.remote_peer_id.write().await = None;
                // Don't send Disconnected here, rely on connection state handler
            }
        }
        Ok(())
    }

    fn setup_ice_candidate_handler(
        pc: &RTCPeerConnection,
        signaling_tx: Arc<RwLock<Option<mpsc::Sender<SignalingMessage>>>>,
        local_id: String,
    ) {
        pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
            let signaling_tx = signaling_tx.clone();
            let local_id = local_id.clone();
            Box::pin(async move {
                if let Some(candidate) = c {
                    if let Ok(candidate_str) = serde_json::to_string(&candidate.to_json().unwrap())
                    {
                        let msg = SignalingMessage {
                            from: local_id,
                            sig_type: SignalingType::Candidate,
                            data: candidate_str,
                        };
                        let tx = {
                            let lock = signaling_tx.read().await;
                            lock.clone()
                        };
                        if let Some(tx) = tx {
                            let _ = tx.send(msg).await;
                        }
                    }
                }
            })
        }));
    }

    fn setup_connection_state_handler(pc: &RTCPeerConnection, event_tx: mpsc::Sender<WebRTCEvent>) {
        let disconnected_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
        pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            let event_tx = event_tx.clone();
            let disconnected_sent = disconnected_sent.clone();
            Box::pin(async move {
                match s {
                    RTCPeerConnectionState::Connected => {
                        let _ = event_tx.send(WebRTCEvent::Connected).await;
                    }
                    RTCPeerConnectionState::Disconnected
                    | RTCPeerConnectionState::Failed
                    | RTCPeerConnectionState::Closed => {
                        if !disconnected_sent.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            let _ = event_tx.send(WebRTCEvent::Disconnected).await;
                        }
                    }
                    _ => {}
                }
            })
        }));
    }

    fn setup_track_handler(
        pc: &Arc<RTCPeerConnection>,
        packet_sink: mpsc::Sender<Bytes>,
        max_depacket_latency: u16,
    ) {
        let pc_weak = Arc::downgrade(pc);
        pc.on_track(Box::new(move |track, _rtp_receiver, rtp_transceiver| {
            if track.kind() == RTPCodecType::Video {
                Self::spawn_pli_loop(pc_weak.clone(), rtp_transceiver.clone(), track.ssrc());
                Self::spawn_video_track_reader(track, packet_sink.clone(), max_depacket_latency);
            }
            Box::pin(async {})
        }));
    }

    fn spawn_pli_loop(
        pc_weak: std::sync::Weak<RTCPeerConnection>,
        rtp_transceiver: Arc<RTCRtpTransceiver>,
        media_ssrc: u32,
    ) {
        tokio::spawn(async move {
            let sender = rtp_transceiver.sender().await;
            let params = sender.get_parameters().await;
            let local_ssrc = params.encodings.first().map(|e| e.ssrc).unwrap_or(0);

            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                if let Some(pc) = pc_weak.upgrade() {
                    let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                        Box::new(PictureLossIndication { sender_ssrc: local_ssrc, media_ssrc });
                    if pc.write_rtcp(&[pli]).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
    }

    fn spawn_video_track_reader(
        track: Arc<TrackRemote>,
        packet_sink: mpsc::Sender<Bytes>,
        max_depacket_latency: u16,
    ) {
        let clock_rate = track.codec().capability.clock_rate;
        tokio::spawn(async move {
            let depacketizer = webrtc::rtp::codecs::h264::H264Packet::default();
            let mut sample_builder =
                SampleBuilder::new(max_depacket_latency, depacketizer, clock_rate);
            while let Ok((rtp, _)) = track.read_rtp().await {
                sample_builder.push(rtp);
                while let Some(sample) = sample_builder.pop() {
                    if packet_sink.send(sample.data).await.is_err() {
                        return;
                    }
                }
            }
        });
    }

    pub fn get_local_id(&self) -> String {
        self.local_peer_id.clone()
    }

    pub async fn write_sample(&self, data: Vec<u8>, duration: Duration) -> WebRTCResult<()> {
        let sample = Sample { data: data.into(), duration, ..Default::default() };
        let track = {
            let lock = self.video_track.read().await;
            lock.clone()
        };
        track.write_sample(&sample).await.map_err(WebRTCError::WriteRTPError)?;
        Ok(())
    }

    pub async fn dial_direct(&self, addr: std::net::SocketAddr) -> WebRTCResult<()> {
        let tx = signaling::dial(addr, self.internal_signal_tx.clone()).await?;
        *self.signaling_tx.write().await = Some(tx);
        Ok(())
    }

    pub async fn create_offer(&self) -> WebRTCResult<()> {
        // Automatically ensure we have a fresh PC before creating an offer
        let pc = self.prepare_pc().await?;

        let offer = pc.create_offer(None).await.map_err(WebRTCError::PeerConnectionError)?;
        let sdp = offer.sdp.clone();
        pc.set_local_description(offer).await.map_err(WebRTCError::PeerConnectionError)?;

        let msg = SignalingMessage {
            from: self.local_peer_id.clone(),
            sig_type: SignalingType::Offer,
            data: sdp,
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            tx.send(msg).await.map_err(WebRTCError::SendError)?;
        }
        Ok(())
    }

    pub async fn accept_call(&self) -> WebRTCResult<()> {
        let pc = self.peer_connection.read().await;
        let answer = pc.create_answer(None).await.map_err(WebRTCError::PeerConnectionError)?;

        let answer_sdp = answer.sdp.clone();
        pc.set_local_description(answer).await.map_err(WebRTCError::PeerConnectionError)?;

        let response = SignalingMessage {
            from: self.local_peer_id.clone(),
            sig_type: SignalingType::Answer,
            data: answer_sdp,
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            tx.send(response).await.map_err(WebRTCError::SendError)?;
        }
        Ok(())
    }

    pub async fn decline_call(&self) -> WebRTCResult<()> {
        let msg = SignalingMessage {
            from: self.local_peer_id.clone(),
            sig_type: SignalingType::Decline,
            data: "".to_string(),
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            tx.send(msg).await.map_err(WebRTCError::SendError)?;
        }

        // Close peer connection
        {
            let pc = self.peer_connection.read().await;
            pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
        }
        *self.remote_peer_id.write().await = None;

        Ok(())
    }

    pub async fn disconnect(&self) -> WebRTCResult<()> {
        // Send a decline message first so the other side doesn't have to wait for timeout
        let msg = SignalingMessage {
            from: self.local_peer_id.clone(),
            sig_type: SignalingType::Decline,
            data: "".to_string(),
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            let _ = tx.send(msg).await;
        }

        {
            let pc = self.peer_connection.read().await;
            pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
        }
        *self.remote_peer_id.write().await = None;
        Ok(())
    }
}
