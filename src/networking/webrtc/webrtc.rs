use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use bytes::Bytes;
use loki_shared::{SignalingMessage, SignalingType};
use tokio::sync::mpsc;
use webrtc::{
    api::{
        APIBuilder,
        media_engine::{MIME_TYPE_H264, MediaEngine},
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    media::Sample,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp::packet::Packet,
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
    track::track_local::{TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP},
};

use crate::networking::{
    signaling,
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

/// Holds the state for the WebRTC connection
#[derive(Clone, Debug)]
pub struct WebRTC {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub signaling_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<TrackLocalStaticRTP>,
}

impl WebRTC {
    pub async fn new() -> WebRTCResult<Self> {
        let (to_webrtc_tx, mut to_webrtc_rx) = mpsc::channel(100);
        let signaling_tx = signaling::connect(to_webrtc_tx).await?;

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

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability { mime_type: MIME_TYPE_H264.to_owned(), ..Default::default() },
            "video".to_owned(),
            "loki".to_owned(),
        ));
        peer_connection
            .add_track(Arc::clone(&video_track)
                as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        // Task to handle incoming signaling messages
        let pc_reader_clone = Arc::clone(&peer_connection);
        tokio::spawn(async move {
            while let Some(msg) = to_webrtc_rx.recv().await {
                if let Err(e) = handle_signaling_message(msg, &pc_reader_clone).await {
                    tracing::error!("Error handling signaling message: {}", e);
                }
            }
            tracing::info!("WebRTC signaling reader task finished.");
        });

        // ICE candidate handling
        let signaling_tx_clone = signaling_tx.clone();
        peer_connection.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
            let signaling_tx = signaling_tx_clone.clone();
            Box::pin(async move {
                if let Some(candidate) = c {
                    match serde_json::to_string(&candidate.to_json().unwrap()) {
                        Ok(candidate_str) => {
                            let msg = SignalingMessage {
                                signalling_type: SignalingType::Candidate,
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
                }
            })
        }));

        // Log the connection state changes only in debug mode
        #[cfg(debug_assertions)]
        peer_connection.on_peer_connection_state_change(Box::new(|s: RTCPeerConnectionState| {
            tracing::debug!("Peer Connection State has changed: {}", s);
            Box::pin(async {})
        }));

        tracing::info!("Peer connection created.");

        Ok(Self { peer_connection, signaling_tx, video_track })
    }

    pub async fn write_frame(
        &self,
        frame_data: Bytes,
        frame_timestamp: SystemTime,
        frame_duration: Duration,
    ) -> WebRTCResult<()> {
        let sample = Sample {
            data: frame_data,
            duration: frame_duration,
            timestamp: frame_timestamp,
            ..Default::default()
        };
        //TODO
        //self.video_track.write_rtp(pkt);
        Ok(())
    }

    pub async fn create_offer(&self) -> WebRTCResult<()> {
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

        let msg = SignalingMessage { signalling_type: SignalingType::Offer, data: sdp };

        self.signaling_tx.send(msg).await.map_err(WebRTCError::SendError)?;

        Ok(())
    }

    pub async fn create_answer(&self) -> WebRTCResult<()> {
        let answer = self
            .peer_connection
            .create_answer(None)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        let sdp = answer.sdp.clone();
        self.peer_connection
            .set_local_description(answer)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        let msg = SignalingMessage { signalling_type: SignalingType::Answer, data: sdp };

        self.signaling_tx.send(msg).await.map_err(WebRTCError::SendError)?;

        Ok(())
    }
}

async fn handle_signaling_message(
    msg: SignalingMessage,
    peer_connection: &Arc<RTCPeerConnection>,
) -> WebRTCResult<()> {
    match msg.signalling_type {
        SignalingType::Offer => {
            let sdp = RTCSessionDescription::offer(msg.data).map_err(WebRTCError::SdpError)?;
            peer_connection
                .set_remote_description(sdp)
                .await
                .map_err(WebRTCError::PeerConnectionError)?;
        }
        SignalingType::Answer => {
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
