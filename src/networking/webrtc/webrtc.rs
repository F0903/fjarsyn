use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use bytes::Bytes;
use fjarsyn_shared::{SignalingMessage, SignalingType};
use tokio::sync::mpsc;
#[cfg(debug_assertions)]
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
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
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

use crate::networking::{
    signaling,
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

#[derive(Debug, Clone)]
pub enum WebRTCEvent {
    Connected,
    Disconnected,
    IncomingCall(String),
}

/// Holds the state for the WebRTC connection
#[derive(Clone, Debug)]
pub struct WebRTC {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub signaling_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<TrackLocalStaticSample>,
    pub remote_peer_id: Arc<RwLock<Option<String>>>,
    pub local_peer_id: Arc<RwLock<Option<String>>>,
}

impl WebRTC {
    const STREAM_ID: &str = "fjarsyn-webrtc";

    pub async fn init(
        signaling_url: String,
        packet_sink: mpsc::Sender<Bytes>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        max_depacket_latency: u16,
    ) -> WebRTCResult<Self> {
        let (signal_tx, mut signal_rx) = mpsc::channel(100);
        let (signaling_tx, id) = signaling::connect(signaling_url, signal_tx).await?;

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
        let peer_connection =
            api.new_peer_connection(config).await.map_err(WebRTCError::PeerConnectionError)?;
        let peer_connection = Arc::new(peer_connection);

        let video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability { mime_type: MIME_TYPE_H264.to_owned(), ..Default::default() },
            "video".to_owned(),
            Self::STREAM_ID.to_owned(),
        ));
        let rtc_rtp_sender = peer_connection
            .add_track(Arc::clone(&video_track)
                as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        tokio::spawn(async move {
            let Ok((packets, _attributes)) = rtc_rtp_sender.read_rtcp().await else {
                tracing::error!("Error reading RTCP packets");
                return;
            };
            for packet in packets {
                tracing::debug!("Received RTCP packet: {:?}", packet);
            }
        });

        let remote_peer_id = Arc::new(RwLock::<Option<String>>::new(None));
        let local_peer_id = Arc::new(RwLock::<Option<String>>::new(Some(id)));

        // Task to handle incoming signaling messages
        let pc_reader_clone = Arc::clone(&peer_connection);
        let remote_peer_id_clone = remote_peer_id.clone();
        let local_peer_id_clone = local_peer_id.clone();
        let signaling_tx_reader = signaling_tx.clone();
        let event_sink_reader = event_tx.clone();

        tokio::spawn(async move {
            while let Some(msg) = signal_rx.recv().await {
                if let Err(e) = handle_signaling_message(
                    msg,
                    pc_reader_clone.clone(),
                    remote_peer_id_clone.clone(),
                    local_peer_id_clone.clone(),
                    signaling_tx_reader.clone(),
                    event_sink_reader.clone(),
                )
                .await
                {
                    tracing::error!("Error handling signaling message: {}", e);
                }
            }
            tracing::info!("WebRTC signaling reader task finished.");
        });

        // ICE candidate handling
        let signaling_tx_clone = signaling_tx.clone();
        let remote_peer_id_ice = remote_peer_id.clone();
        peer_connection.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
            let signaling_tx = signaling_tx_clone.clone();
            let remote_peer_id = remote_peer_id_ice.clone();
            Box::pin(async move {
                let Some(candidate) = c else {
                    return;
                };

                match serde_json::to_string(&candidate.to_json().unwrap()) {
                    Ok(candidate_str) => {
                        let Some(remote_id) = remote_peer_id.read().unwrap().clone() else {
                            return;
                        };
                        let msg = SignalingMessage {
                            to: remote_id,
                            from: String::new(),
                            sig_type: SignalingType::Candidate,
                            data: candidate_str,
                        };
                        if let Err(e) = signaling_tx.send(msg).await {
                            tracing::error!("Failed to send ICE candidate: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to serialize ICE candidate: {}", e);
                    }
                }
            })
        }));

        let event_sink_state = event_tx.clone();
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                tracing::debug!("Peer Connection State has changed: {}", s);
                let event_sink = event_sink_state.clone();
                Box::pin(async move {
                    if s == RTCPeerConnectionState::Connected {
                        if let Err(e) = event_sink.send(WebRTCEvent::Connected).await {
                            tracing::error!("Failed to send Connected event: {}", e);
                        }
                    } else if s == RTCPeerConnectionState::Disconnected
                        || s == RTCPeerConnectionState::Closed
                    {
                        let _ = event_sink.send(WebRTCEvent::Disconnected).await;
                    }
                })
            },
        ));

        #[cfg(debug_assertions)]
        peer_connection.on_ice_connection_state_change(Box::new(|s: RTCIceConnectionState| {
            tracing::debug!("ICE Connection State has changed: {}", s);
            Box::pin(async {})
        }));

        let pc = Arc::downgrade(&peer_connection);
        peer_connection.on_track(Box::new(move |track, _rtp_receiver, rtp_transceiver| {
            tracing::debug!("Received track: {}", track.id());

            let media_ssrc = track.ssrc();

            match track.kind() {
                RTPCodecType::Video => {
                    let pc = pc.clone();
                    let packet_sink = packet_sink.clone();
                    let rtp_transceiver = rtp_transceiver.clone();

                    // We just send a PLI every 3 seconds for now.
                    tokio::spawn(async move {
                        // Get the local SSRC from the transceiver
                        let sender = rtp_transceiver.sender().await;
                        let params = sender.get_parameters().await;
                        let local_ssrc = params.encodings.first().map(|e| e.ssrc).unwrap_or(0);

                        const PLI_INTERVAL: u64 = 3;
                        let mut result = Result::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(PLI_INTERVAL));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() => {
                                    if let Some(pc) = pc.upgrade() {
                                        result = pc.write_rtcp(&[Box::new(PictureLossIndication {sender_ssrc: local_ssrc, media_ssrc})]).await;
                                    } else {
                                        break;
                                    }
                                }
                            };
                        }
                    });

                    tokio::spawn(async move {
                        tracing::debug!("Track with type '{}' starting...", track.codec().capability.mime_type);

                        let depacketizer = webrtc::rtp::codecs::h264::H264Packet::default();
                        let mut sample_builder = SampleBuilder::new(max_depacket_latency, depacketizer, track.codec().capability.clock_rate);

                        while let Ok((rtp, _attributes)) = track.read_rtp().await {
                            sample_builder.push(rtp);
                            while let Some(sample) = sample_builder.pop() {
                                if let Err(e) = packet_sink.send(sample.data).await {
                                    tracing::error!("Failed to send received frame to sink: {}", e);
                                    return;
                                }
                            }
                        }

                        tracing::debug!("Track with type '{}' finished.", track.codec().capability.mime_type);
                    });

                }
                _ => {
                    tracing::warn!("Received non-video track");
                }
            }

            Box::pin(async {})
        }));

        tracing::info!("Peer connection created.");

        Ok(Self { peer_connection, signaling_tx, video_track, remote_peer_id, local_peer_id })
    }

    pub fn get_local_id(&self) -> Option<String> {
        self.local_peer_id.read().unwrap().clone()
    }

    pub fn get_remote_id(&self) -> Option<String> {
        self.remote_peer_id.read().unwrap().clone()
    }

    pub async fn write_sample(&self, data: Vec<u8>, duration: Duration) -> WebRTCResult<()> {
        let sample = Sample { data: data.into(), duration, ..Default::default() };
        self.video_track.write_sample(&sample).await.map_err(WebRTCError::WriteRTPError)?;
        Ok(())
    }

    pub async fn create_offer(&self, target_id: String) -> WebRTCResult<()> {
        let offer = self
            .peer_connection
            .create_offer(None)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        let sdp = offer.sdp.clone();
        self.peer_connection
            .set_local_description(offer)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        // Update remote ID since we are initiating call to them
        *self.remote_peer_id.write().unwrap() = Some(target_id.clone());

        let msg = SignalingMessage {
            to: target_id,
            from: String::new(),
            sig_type: SignalingType::Offer,
            data: sdp,
        };

        self.signaling_tx.send(msg).await.map_err(WebRTCError::SendError)?;

        Ok(())
    }

    pub async fn disconnect(&self) -> WebRTCResult<()> {
        self.peer_connection.close().await.map_err(WebRTCError::PeerConnectionError)?;
        *self.remote_peer_id.write().unwrap() = None;
        Ok(())
    }
}

async fn handle_signaling_message(
    msg: SignalingMessage,
    peer_connection: Arc<RTCPeerConnection>,
    remote_peer_id: Arc<RwLock<Option<String>>>,
    local_peer_id: Arc<RwLock<Option<String>>>,
    signaling_tx: mpsc::Sender<SignalingMessage>,
    event_sink: mpsc::Sender<WebRTCEvent>,
) -> WebRTCResult<()> {
    match msg.sig_type {
        SignalingType::Identity => {
            tracing::info!("Server assigned identity: {}", msg.data);
            *local_peer_id.write().unwrap() = Some(msg.data);
        }
        SignalingType::Offer => {
            // Lock onto the sender
            *remote_peer_id.write().unwrap() = Some(msg.from.clone());
            tracing::info!("Received Offer from {}", msg.from);

            // Notify UI of incoming call
            if let Err(e) = event_sink.send(WebRTCEvent::IncomingCall(msg.from.clone())).await {
                tracing::error!("Failed to send IncomingCall event: {}", e);
            }

            let sdp = RTCSessionDescription::offer(msg.data).map_err(WebRTCError::SdpError)?;
            peer_connection
                .set_remote_description(sdp)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;

            // Auto-Answer logic
            let answer = peer_connection
                .create_answer(None)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;

            let answer_sdp = answer.sdp.clone();
            peer_connection
                .set_local_description(answer)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;

            let response_msg = SignalingMessage {
                to: msg.from,
                from: String::new(),
                sig_type: SignalingType::Answer,
                data: answer_sdp,
            };

            signaling_tx.send(response_msg).await.map_err(WebRTCError::SendError)?;
        }
        SignalingType::Answer => {
            // Lock onto the sender (if not already?)
            *remote_peer_id.write().unwrap() = Some(msg.from.clone());
            tracing::info!("Received Answer from {}", msg.from);

            let sdp = RTCSessionDescription::answer(msg.data).map_err(WebRTCError::SdpError)?;
            peer_connection
                .set_remote_description(sdp)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;
        }
        SignalingType::Candidate => {
            let candidate: RTCIceCandidateInit =
                serde_json::from_str(&msg.data).map_err(WebRTCError::DeserializeError)?;
            peer_connection
                .add_ice_candidate(candidate)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;
        }
    }
    Ok(())
}
