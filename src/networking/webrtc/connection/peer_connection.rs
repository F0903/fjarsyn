use std::sync::Arc;

use webrtc::{
    api::{
        APIBuilder,
        media_engine::{MIME_TYPE_H264, MediaEngine},
    },
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

use super::WebRTC;
use crate::networking::webrtc::{WebRTCError, webrtc_error::WebRTCResult};

impl WebRTC {
    pub(super) async fn create_pc() -> WebRTCResult<RTCPeerConnection> {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().map_err(WebRTCError::CodecError)?;
        let api = APIBuilder::new().with_media_engine(media_engine).build();
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };
        api.new_peer_connection(config).await.map_err(WebRTCError::PeerConnectionError)
    }

    pub(super) async fn prepare_pc(&self) -> WebRTCResult<Arc<RTCPeerConnection>> {
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

                *self.peer_connection.write().await = new_pc.clone();
                *self.video_track.write().await = new_video_track;
                self.clear_control_channel().await;
                *self.remote_peer_id.write().await = None;

                self.setup_pc_handlers(&new_pc).await;

                Ok(new_pc)
            }
            _ => Ok(current_pc),
        }
    }

    pub(super) async fn setup_pc_handlers(&self, pc: &Arc<RTCPeerConnection>) {
        Self::setup_ice_candidate_handler(
            pc,
            self.signaling_tx.clone(),
            self.local_peer_id.clone(),
        );
        Self::setup_connection_state_handler(pc, self.event_tx.clone());
        Self::setup_control_channel_handler(
            pc,
            self.control_channel.clone(),
            self.event_tx.clone(),
        );
        Self::setup_track_handler(
            pc,
            self.packet_sink.clone(),
            self.max_depacket_latency,
            self.tasks.clone(),
        );
    }
}
