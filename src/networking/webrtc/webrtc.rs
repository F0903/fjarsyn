use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use loki_shared::{SignalingMessage, SignalingType};
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
    media::Sample,
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

/// Holds the state for the WebRTC connection
#[derive(Clone, Debug)]
pub struct WebRTC {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub signaling_tx: mpsc::Sender<SignalingMessage>,
    pub video_track: Arc<TrackLocalStaticSample>,
}

impl WebRTC {
    const STREAM_ID: &str = "loki";

    pub async fn new() -> WebRTCResult<Self> {
        let (signal_tx, mut signal_rx) = mpsc::channel(100);
        let signaling_tx = signaling::connect(signal_tx).await?;

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

        // Task to handle incoming signaling messages
        let pc_reader_clone = Arc::clone(&peer_connection);
        tokio::spawn(async move {
            while let Some(msg) = signal_rx.recv().await {
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

        #[cfg(debug_assertions)]
        peer_connection.on_ice_connection_state_change(Box::new(|s: RTCIceConnectionState| {
            tracing::debug!("ICE Connection State has changed: {}", s);
            Box::pin(async {})
        }));

        let pc = Arc::downgrade(&peer_connection);
        let video_track_arc = video_track.clone();
        peer_connection.on_track(Box::new(move |track, _rtp_receiver, _rtp_transceiver| {
            tracing::debug!("Received track: {}", track.id());

            //NOTE: this currently follows the implementation in the reflect.rs example

            // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
            // This is a temporary fix until we implement incoming RTCP events, then we would push a PLI only when a viewer requests it
            let media_ssrc = track.ssrc();


            match track.kind() {
                RTPCodecType::Video => {
                    let pc = pc.clone();

                    tokio::spawn(async move {
                        let mut result = Result::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(3));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() => {
                                    if let Some(pc) = pc.upgrade() {
                                        result = pc.write_rtcp(&[Box::new(PictureLossIndication {sender_ssrc: 0, media_ssrc})]).await;
                                    } else {
                                        break;
                                    }
                                }
                            };
                        }
                    });


                    let video_track_arc = video_track_arc.clone();
                    tokio::spawn(async move {
                        tracing::debug!("Track with type '{}' starting...", track.codec().capability.mime_type);

                        while let Ok((rtp, _attributes)) = track.read_rtp().await {
                            //TODO: this is most likely implemented wrong
                            if let Err(error) = video_track_arc.write_sample(&Sample {
                                data: rtp.payload,
                                ..Default::default()
                            }).await {
                                tracing::error!("Failed to write sample from remote track to local track: {}", error);
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

        Ok(Self { peer_connection, signaling_tx, video_track })
    }

    pub async fn write_frame(
        &self,
        frame_data: Bytes,
        frame_duration: Duration,
    ) -> WebRTCResult<()> {
        let sample = Sample { data: frame_data, duration: frame_duration, ..Default::default() };
        self.video_track.write_sample(&sample).await.map_err(WebRTCError::WriteRTPError)?;
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
