use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinSet,
};
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_H264, MediaEngine},
    },
    data_channel::RTCDataChannel,
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    peer_connection::{RTCPeerConnection, configuration::RTCConfiguration},
    rtp_transceiver::{
        RTCPFeedback,
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    },
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

use super::{
    super::{Config, Event, EventDispatcher, share_epoch},
    ice_credentials::IceCredentials,
    rtc_operation,
};
use crate::peer_session::{Error, TransportGeneration, media::RemoteVideoSample};

fn register_h264_codecs(media_engine: &mut MediaEngine) -> Result<(), Error> {
    let rtcp_feedback = vec![
        RTCPFeedback { typ: "goog-remb".into(), parameter: String::new() },
        RTCPFeedback { typ: "ccm".into(), parameter: "fir".into() },
        RTCPFeedback { typ: "nack".into(), parameter: String::new() },
        RTCPFeedback { typ: "nack".into(), parameter: "pli".into() },
    ];
    for (payload_type, profile_level_id) in [(102, "42001f"), (125, "42e01f"), (123, "640032")] {
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_H264.to_owned(),
                        clock_rate: 90_000,
                        sdp_fmtp_line: format!(
                            "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id={profile_level_id}"
                        ),
                        rtcp_feedback: rtcp_feedback.clone(),
                        ..Default::default()
                    },
                    payload_type,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|error| Error::WebRtc(error.to_string()))?;
    }
    Ok(())
}

pub(in crate::peer_session) struct Peer {
    pub(super) pc: Arc<RTCPeerConnection>,
    pub(super) video_track: Arc<TrackLocalStaticSample>,
    pub(super) control: Option<Arc<RTCDataChannel>>,
    pub(super) messaging: Option<Arc<RTCDataChannel>>,
    pub(super) generation: TransportGeneration,
    pub(super) callback_generation: Arc<AtomicU64>,
    pub(super) local_description_generation: Option<TransportGeneration>,
    pub(super) remote_description_generation: Option<TransportGeneration>,
    pub(super) local_ice_credentials: Option<IceCredentials>,
    pub(super) remote_ice_credentials: Option<IceCredentials>,
    pub(super) share_epoch_extension_id: Option<u8>,
    pub(super) max_candidates_per_generation: usize,
    pub(super) remote_candidates_received: usize,
    pub(super) max_data_message_bytes: usize,
    pub(super) operation_timeout: Duration,
    pub(super) max_depacket_latency: Duration,
    pub(super) remote_video_tx: broadcast::Sender<RemoteVideoSample>,
    pub(super) events: EventDispatcher,
    pub(super) tasks: JoinSet<()>,
    pub(super) remote_video_claimed: bool,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("connection_state", &self.pc.connection_state())
            .field("has_control", &self.control.is_some())
            .field("has_messaging", &self.messaging.is_some())
            .finish_non_exhaustive()
    }
}

impl Peer {
    const STREAM_ID: &str = "fjarsyn-screen-v1";

    pub(in crate::peer_session) async fn new(
        config: Config,
        event_tx: mpsc::Sender<Event>,
        fatal_tx: watch::Sender<Option<String>>,
        remote_video_tx: broadcast::Sender<RemoteVideoSample>,
    ) -> Result<Self, Error> {
        let mut media_engine = MediaEngine::default();
        register_h264_codecs(&mut media_engine)?;
        share_epoch::register(&mut media_engine)?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|error| Error::WebRtc(error.to_string()))?;
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();
        let pc = Arc::new(
            rtc_operation(
                config.operation_timeout,
                api.new_peer_connection(RTCConfiguration {
                    ice_servers: config
                        .ice_servers
                        .iter()
                        .cloned()
                        .map(|url| RTCIceServer { urls: vec![url], ..Default::default() })
                        .collect(),
                    ..Default::default()
                }),
            )
            .await?,
        );

        let video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability { mime_type: MIME_TYPE_H264.to_owned(), ..Default::default() },
            "screen".to_owned(),
            Self::STREAM_ID.to_owned(),
        ));
        if let Err(error) = rtc_operation(
            config.operation_timeout,
            pc.add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>),
        )
        .await
        {
            // A partially constructed peer connection owns internal WebRTC tasks.
            // Close it explicitly before returning the construction error.
            let _ = tokio::time::timeout(config.operation_timeout, pc.close()).await;
            return Err(error);
        }

        let events = EventDispatcher::new(event_tx, fatal_tx);
        let callback_generation = Arc::new(AtomicU64::new(TransportGeneration::INITIAL.value()));
        Self::register_peer_callbacks(&pc, events.clone(), callback_generation.clone());

        Ok(Self {
            pc,
            video_track,
            control: None,
            messaging: None,
            generation: TransportGeneration::INITIAL,
            callback_generation,
            local_description_generation: None,
            remote_description_generation: None,
            local_ice_credentials: None,
            remote_ice_credentials: None,
            share_epoch_extension_id: None,
            max_candidates_per_generation: config.max_candidates_per_generation.max(1),
            remote_candidates_received: 0,
            max_data_message_bytes: config.max_data_message_bytes.max(1),
            operation_timeout: config.operation_timeout,
            max_depacket_latency: config.max_depacket_latency,
            remote_video_tx,
            events,
            tasks: JoinSet::new(),
            remote_video_claimed: false,
        })
    }

    #[cfg(test)]
    async fn shutdown(self) {
        let deadline = tokio::time::Instant::now() + self.operation_timeout;
        self.shutdown_until(deadline).await;
    }

    pub(in crate::peer_session) async fn shutdown_until(mut self, deadline: tokio::time::Instant) {
        let control = self.control.take();
        let messaging = self.messaging.take();
        let pc = Arc::clone(&self.pc);
        let close_transports = async move {
            tokio::join!(
                async move {
                    if let Some(channel) = control {
                        let _ = channel.close().await;
                    }
                },
                async move {
                    if let Some(channel) = messaging {
                        let _ = channel.close().await;
                    }
                },
                async move {
                    let _ = pc.close().await;
                },
            );
        };
        let _ = tokio::time::timeout_at(deadline, close_transports).await;
        self.tasks.abort_all();
        while !self.tasks.is_empty() && tokio::time::Instant::now() < deadline {
            match tokio::time::timeout_at(deadline, self.tasks.join_next()).await {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

    use super::{
        super::{
            super::ChannelKind, data_channels::validate_inbound_data_frame,
            video::claim_remote_video_track,
        },
        *,
    };

    fn sdp_attribute(sdp: &str, prefix: &str) -> String {
        sdp.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing SDP attribute {prefix}"))
            .trim()
            .to_owned()
    }

    #[tokio::test]
    async fn remote_ice_candidates_require_sdp_credentials_and_are_bounded() {
        let (event_tx, _event_rx) = mpsc::channel(8);
        let (fatal_tx, _fatal_rx) = watch::channel(None);
        let (video_tx, _) = broadcast::channel(2);
        let (offer_event_tx, _offer_event_rx) = mpsc::channel(8);
        let (offer_fatal_tx, _offer_fatal_rx) = watch::channel(None);
        let (offer_video_tx, _) = broadcast::channel(2);
        let config = Config {
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            max_candidates_per_generation: 1,
            max_data_message_bytes: 16 * 1024,
            operation_timeout: Duration::from_secs(1),
        };
        let mut peer = Peer::new(config.clone(), event_tx, fatal_tx, video_tx).await.unwrap();
        let candidate = RTCIceCandidateInit {
            candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".into(),
            ..Default::default()
        };

        assert!(matches!(
            peer.add_remote_candidate(TransportGeneration::INITIAL, candidate.clone()).await,
            Err(Error::Protocol(_))
        ));
        let mut offerer =
            Peer::new(config, offer_event_tx, offer_fatal_tx, offer_video_tx).await.unwrap();
        let offer = offerer.create_offer().await.unwrap();
        let remote_ufrag = IceCredentials::from_sdp(&offer).unwrap().username_fragment().to_owned();
        peer.apply_offer_and_create_answer(offer).await.unwrap();
        let candidate = RTCIceCandidateInit { username_fragment: Some(remote_ufrag), ..candidate };
        peer.add_remote_candidate(TransportGeneration::INITIAL, candidate.clone()).await.unwrap();
        assert!(matches!(
            peer.add_remote_candidate(TransportGeneration::INITIAL, candidate).await,
            Err(Error::Protocol(_))
        ));
        peer.shutdown().await;
        offerer.shutdown().await;
    }

    #[test]
    fn data_channel_frames_are_text_and_size_bounded_before_enqueue() {
        assert!(validate_inbound_data_frame(ChannelKind::Control, false, b"x", 8).is_err());
        assert!(validate_inbound_data_frame(ChannelKind::Control, true, &[0xff], 8).is_err());
        assert!(
            validate_inbound_data_frame(ChannelKind::Messaging, true, b"123456789", 8).is_err()
        );
        assert!(validate_inbound_data_frame(ChannelKind::Messaging, true, b"valid", 8).is_ok());
    }

    #[test]
    fn remote_media_is_exactly_one_h264_track() {
        let mut claimed = false;
        assert!(claim_remote_video_track(&mut claimed, "video/VP8").is_err());
        assert!(!claimed);
        assert!(claim_remote_video_track(&mut claimed, "video/H264").is_ok());
        assert!(claim_remote_video_track(&mut claimed, "video/H264").is_err());
    }

    #[tokio::test]
    async fn offers_only_advertise_h264_video() {
        let (event_tx, _event_rx) = mpsc::channel(8);
        let (fatal_tx, _fatal_rx) = watch::channel(None);
        let (video_tx, _) = broadcast::channel(2);
        let mut peer = Peer::new(
            Config {
                ice_servers: Vec::new(),
                max_depacket_latency: Duration::from_millis(100),
                max_candidates_per_generation: 1,
                max_data_message_bytes: 16 * 1024,
                operation_timeout: Duration::from_secs(1),
            },
            event_tx,
            fatal_tx,
            video_tx,
        )
        .await
        .unwrap();

        let offer = peer.create_offer().await.unwrap();
        assert!(offer.contains("H264/90000"));
        assert!(!offer.contains("VP8/90000"));
        assert!(!offer.contains("VP9/90000"));
        assert!(!offer.contains("AV1/90000"));
        peer.shutdown().await;
    }

    #[tokio::test]
    async fn restart_offer_rotates_ice_credentials_without_recreating_capabilities() {
        let (offer_events, _offer_event_rx) = mpsc::channel(128);
        let (offer_fatal, _offer_fatal_rx) = watch::channel(None);
        let (offer_video, _) = broadcast::channel(2);
        let (answer_events, _answer_event_rx) = mpsc::channel(128);
        let (answer_fatal, _answer_fatal_rx) = watch::channel(None);
        let (answer_video, _) = broadcast::channel(2);
        let config = Config {
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            max_candidates_per_generation: 8,
            max_data_message_bytes: 16 * 1024,
            operation_timeout: Duration::from_secs(5),
        };
        let mut offerer =
            Peer::new(config.clone(), offer_events, offer_fatal, offer_video).await.unwrap();
        let mut answerer =
            Peer::new(config, answer_events, answer_fatal, answer_video).await.unwrap();

        offerer.prepare_offerer_channels().await.unwrap();
        let initial_offer = offerer.create_offer().await.unwrap();
        let initial_offer_replay = initial_offer.clone();
        let initial_fingerprint = sdp_attribute(&initial_offer, "a=fingerprint:");
        let initial_ufrag = sdp_attribute(&initial_offer, "a=ice-ufrag:");
        let initial_answer = answerer.apply_offer_and_create_answer(initial_offer).await.unwrap();
        let initial_answer_ufrag = sdp_attribute(&initial_answer, "a=ice-ufrag:");
        offerer.apply_answer(initial_answer).await.unwrap();

        let generation = TransportGeneration::INITIAL.next().unwrap();
        assert!(
            answerer
                .apply_restart_offer_and_create_answer(generation, initial_offer_replay)
                .await
                .is_err()
        );
        assert_eq!(answerer.generation, TransportGeneration::INITIAL);
        let restart_offer = offerer.create_restart_offer(generation).await.unwrap();
        assert_ne!(sdp_attribute(&restart_offer, "a=ice-ufrag:"), initial_ufrag);
        assert_eq!(sdp_attribute(&restart_offer, "a=fingerprint:"), initial_fingerprint);
        assert!(restart_offer.contains("m=application"));
        assert!(restart_offer.contains("m=video"));
        assert!(offerer.control.is_some());
        assert!(offerer.messaging.is_some());

        let restart_answer = answerer
            .apply_restart_offer_and_create_answer(generation, restart_offer)
            .await
            .unwrap();
        assert_ne!(sdp_attribute(&restart_answer, "a=ice-ufrag:"), initial_answer_ufrag);
        offerer.apply_restart_answer(generation, restart_answer).await.unwrap();
        assert_eq!(offerer.generation, generation);
        assert_eq!(answerer.generation, generation);

        offerer.shutdown().await;
        answerer.shutdown().await;
    }
}
