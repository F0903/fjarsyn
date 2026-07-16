use std::{sync::Arc, time::Duration};

use bytes::Bytes;
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
    data_channel::{
        RTCDataChannel, data_channel_init::RTCDataChannelInit,
        data_channel_message::DataChannelMessage,
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    media::{Sample, io::sample_builder::SampleBuilder},
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        RTCPFeedback, RTCRtpTransceiver,
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    },
    track::{
        track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
        track_remote::TrackRemote,
    },
};

use super::{EncodedVideoSample, PeerSessionError};

pub(crate) const CONTROL_CHANNEL_LABEL: &str = "fjarsyn-control-v1";
pub(crate) const MESSAGING_CHANNEL_LABEL: &str = "fjarsyn-messaging-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelKind {
    Control,
    Messaging,
}

impl ChannelKind {
    fn from_label(label: &str) -> Option<Self> {
        match label {
            CONTROL_CHANNEL_LABEL => Some(Self::Control),
            MESSAGING_CHANNEL_LABEL => Some(Self::Messaging),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Control => CONTROL_CHANNEL_LABEL,
            Self::Messaging => MESSAGING_CHANNEL_LABEL,
        }
    }
}

pub(crate) enum RtcEvent {
    LocalCandidate(RTCIceCandidateInit),
    PeerState(RTCPeerConnectionState),
    DataChannel(Arc<RTCDataChannel>),
    ChannelOpen(ChannelKind),
    ChannelClosed(ChannelKind),
    ChannelMessage(ChannelKind, Bytes),
    RemoteTrack(Arc<TrackRemote>, Arc<RTCRtpTransceiver>),
    Error(String),
    ProtocolError(String),
}

#[derive(Clone)]
struct RtcEventDispatcher {
    tx: mpsc::Sender<RtcEvent>,
    fatal_tx: watch::Sender<Option<String>>,
}

impl RtcEventDispatcher {
    fn dispatch(&self, event: RtcEvent) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.fatal_tx.send_replace(Some("WebRTC event queue overflowed".into()));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RtcConfig {
    pub ice_servers: Vec<String>,
    pub max_depacket_latency: Duration,
    pub max_pending_candidates: usize,
    pub max_data_message_bytes: usize,
    pub operation_timeout: Duration,
}

pub(crate) struct RtcPeer {
    pc: Arc<RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticSample>,
    control: Option<Arc<RTCDataChannel>>,
    messaging: Option<Arc<RTCDataChannel>>,
    remote_description_set: bool,
    pending_candidates: Vec<RTCIceCandidateInit>,
    max_pending_candidates: usize,
    max_data_message_bytes: usize,
    operation_timeout: Duration,
    max_depacket_latency: Duration,
    remote_video_tx: broadcast::Sender<EncodedVideoSample>,
    events: RtcEventDispatcher,
    tasks: JoinSet<()>,
    remote_video_claimed: bool,
}

impl std::fmt::Debug for RtcPeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtcPeer")
            .field("connection_state", &self.pc.connection_state())
            .field("has_control", &self.control.is_some())
            .field("has_messaging", &self.messaging.is_some())
            .finish_non_exhaustive()
    }
}

impl RtcPeer {
    const STREAM_ID: &str = "fjarsyn-screen-v1";
    const SAMPLE_BUILDER_PACKET_WINDOW: u16 = 4096;

    pub(crate) async fn new(
        config: RtcConfig,
        event_tx: mpsc::Sender<RtcEvent>,
        fatal_tx: watch::Sender<Option<String>>,
        remote_video_tx: broadcast::Sender<EncodedVideoSample>,
    ) -> Result<Self, PeerSessionError> {
        let mut media_engine = MediaEngine::default();
        register_h264_codecs(&mut media_engine)?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
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

        let events = RtcEventDispatcher { tx: event_tx, fatal_tx };
        Self::register_peer_callbacks(&pc, events.clone());

        Ok(Self {
            pc,
            video_track,
            control: None,
            messaging: None,
            remote_description_set: false,
            pending_candidates: Vec::new(),
            max_pending_candidates: config.max_pending_candidates.max(1),
            max_data_message_bytes: config.max_data_message_bytes.max(1),
            operation_timeout: config.operation_timeout,
            max_depacket_latency: config.max_depacket_latency,
            remote_video_tx,
            events,
            tasks: JoinSet::new(),
            remote_video_claimed: false,
        })
    }

    fn register_peer_callbacks(pc: &Arc<RTCPeerConnection>, events: RtcEventDispatcher) {
        let candidate_events = events.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let candidate_events = candidate_events.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                match candidate.to_json() {
                    Ok(candidate) => candidate_events.dispatch(RtcEvent::LocalCandidate(candidate)),
                    Err(error) => candidate_events.dispatch(RtcEvent::Error(error.to_string())),
                }
            })
        }));

        let state_events = events.clone();
        pc.on_peer_connection_state_change(Box::new(move |state| {
            let state_events = state_events.clone();
            Box::pin(async move {
                state_events.dispatch(RtcEvent::PeerState(state));
            })
        }));

        let channel_events = events.clone();
        pc.on_data_channel(Box::new(move |channel| {
            let channel_events = channel_events.clone();
            Box::pin(async move {
                channel_events.dispatch(RtcEvent::DataChannel(channel));
            })
        }));

        pc.on_track(Box::new(move |track, _receiver, transceiver| {
            let events = events.clone();
            Box::pin(async move {
                if track.kind() == RTPCodecType::Video {
                    events.dispatch(RtcEvent::RemoteTrack(track, transceiver));
                } else {
                    events.dispatch(RtcEvent::ProtocolError(
                        "unexpected non-video media track".into(),
                    ));
                }
            })
        }));
    }

    pub(crate) async fn prepare_offerer_channels(&mut self) -> Result<(), PeerSessionError> {
        for kind in [ChannelKind::Control, ChannelKind::Messaging] {
            let options = RTCDataChannelInit {
                ordered: Some(true),
                protocol: Some(kind.label().to_owned()),
                ..Default::default()
            };
            let channel = rtc_operation(
                self.operation_timeout,
                self.pc.create_data_channel(kind.label(), Some(options)),
            )
            .await?;
            self.attach_data_channel(channel)?;
        }
        Ok(())
    }

    pub(crate) fn attach_data_channel(
        &mut self,
        channel: Arc<RTCDataChannel>,
    ) -> Result<(), PeerSessionError> {
        let kind = ChannelKind::from_label(channel.label()).ok_or_else(|| {
            PeerSessionError::Protocol(format!("unexpected data channel {}", channel.label()))
        })?;
        if !channel.ordered()
            || channel.max_packet_lifetime().is_some()
            || channel.max_retransmits().is_some()
            || channel.protocol() != kind.label()
        {
            return Err(PeerSessionError::Protocol(format!(
                "data channel {} is not reliable and ordered",
                channel.label()
            )));
        }
        let slot = match kind {
            ChannelKind::Control => &mut self.control,
            ChannelKind::Messaging => &mut self.messaging,
        };
        if slot.is_some() {
            return Err(PeerSessionError::Protocol(format!(
                "duplicate data channel {}",
                channel.label()
            )));
        }

        let open_events = self.events.clone();
        channel.on_open(Box::new(move || {
            let open_events = open_events.clone();
            Box::pin(async move {
                open_events.dispatch(RtcEvent::ChannelOpen(kind));
            })
        }));
        let close_events = self.events.clone();
        channel.on_close(Box::new(move || {
            let close_events = close_events.clone();
            Box::pin(async move {
                close_events.dispatch(RtcEvent::ChannelClosed(kind));
            })
        }));
        let message_events = self.events.clone();
        let max_data_message_bytes = self.max_data_message_bytes;
        channel.on_message(Box::new(move |message: DataChannelMessage| {
            let message_events = message_events.clone();
            Box::pin(async move {
                if let Err(reason) = validate_inbound_data_frame(
                    kind,
                    message.is_string,
                    &message.data,
                    max_data_message_bytes,
                ) {
                    message_events.dispatch(RtcEvent::ProtocolError(reason));
                } else {
                    message_events.dispatch(RtcEvent::ChannelMessage(kind, message.data));
                }
            })
        }));
        *slot = Some(channel);
        Ok(())
    }

    pub(crate) async fn create_offer(&self) -> Result<String, PeerSessionError> {
        let offer = rtc_operation(self.operation_timeout, self.pc.create_offer(None)).await?;
        let sdp = offer.sdp.clone();
        rtc_operation(self.operation_timeout, self.pc.set_local_description(offer)).await?;
        Ok(sdp)
    }

    pub(crate) async fn apply_offer_and_create_answer(
        &mut self,
        sdp: String,
    ) -> Result<String, PeerSessionError> {
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(offer).await?;
        let answer = rtc_operation(self.operation_timeout, self.pc.create_answer(None)).await?;
        let answer_sdp = answer.sdp.clone();
        rtc_operation(self.operation_timeout, self.pc.set_local_description(answer)).await?;
        Ok(answer_sdp)
    }

    pub(crate) async fn apply_answer(&mut self, sdp: String) -> Result<(), PeerSessionError> {
        let answer = RTCSessionDescription::answer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(answer).await
    }

    async fn set_remote_description(
        &mut self,
        description: RTCSessionDescription,
    ) -> Result<(), PeerSessionError> {
        let deadline = tokio::time::Instant::now() + self.operation_timeout;
        rtc_operation_until(deadline, self.pc.set_remote_description(description)).await?;
        self.remote_description_set = true;
        for candidate in std::mem::take(&mut self.pending_candidates) {
            rtc_operation_until(deadline, self.pc.add_ice_candidate(candidate)).await?;
        }
        Ok(())
    }

    pub(crate) async fn add_remote_candidate(
        &mut self,
        candidate: RTCIceCandidateInit,
    ) -> Result<(), PeerSessionError> {
        if !self.remote_description_set {
            if self.pending_candidates.len() >= self.max_pending_candidates {
                return Err(PeerSessionError::Protocol("too many early ICE candidates".into()));
            }
            self.pending_candidates.push(candidate);
            return Ok(());
        }
        rtc_operation(self.operation_timeout, self.pc.add_ice_candidate(candidate)).await
    }

    pub(crate) fn start_remote_track(
        &mut self,
        track: Arc<TrackRemote>,
        transceiver: Arc<RTCRtpTransceiver>,
    ) -> Result<(), PeerSessionError> {
        let codec = track.codec();
        claim_remote_video_track(&mut self.remote_video_claimed, &codec.capability.mime_type)?;
        let remote_video_tx = self.remote_video_tx.clone();
        let max_delay = self.max_depacket_latency;
        let media_ssrc = track.ssrc();
        self.tasks.spawn(async move {
            let clock_rate = codec.capability.clock_rate;
            let depacketizer = webrtc::rtp::codecs::h264::H264Packet::default();
            let mut builder =
                SampleBuilder::new(Self::SAMPLE_BUILDER_PACKET_WINDOW, depacketizer, clock_rate)
                    .with_max_time_delay(max_delay);

            while let Ok((packet, _)) = track.read_rtp().await {
                builder.push(packet);
                while let Some(sample) = builder.pop() {
                    let _ = remote_video_tx
                        .send(EncodedVideoSample { data: sample.data, duration: sample.duration });
                }
            }
        });

        let pc = Arc::downgrade(&self.pc);
        self.tasks.spawn(async move {
            let sender = transceiver.sender().await;
            let parameters = sender.get_parameters().await;
            let sender_ssrc = parameters.encodings.first().map(|value| value.ssrc).unwrap_or(0);
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            loop {
                interval.tick().await;
                let Some(pc) = pc.upgrade() else { break };
                let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                    Box::new(PictureLossIndication { sender_ssrc, media_ssrc });
                if pc.write_rtcp(&[pli]).await.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    pub(crate) async fn send_control(&self, data: String) -> Result<(), PeerSessionError> {
        send_data(self.control.as_ref(), data, "control", self.operation_timeout).await
    }

    pub(crate) async fn send_message(&self, data: String) -> Result<(), PeerSessionError> {
        send_data(self.messaging.as_ref(), data, "messaging", self.operation_timeout).await
    }

    pub(crate) async fn write_video(
        &self,
        sample: EncodedVideoSample,
    ) -> Result<(), PeerSessionError> {
        tokio::time::timeout(
            self.operation_timeout,
            self.video_track.write_sample(&Sample {
                data: sample.data,
                duration: sample.duration,
                ..Default::default()
            }),
        )
        .await
        .map_err(|_| PeerSessionError::OperationTimeout)?
        .map_err(|error| PeerSessionError::WebRtc(error.to_string()))
    }

    #[cfg(test)]
    pub(crate) async fn shutdown(self) {
        let deadline = tokio::time::Instant::now() + self.operation_timeout;
        self.shutdown_until(deadline).await;
    }

    pub(crate) async fn shutdown_until(mut self, deadline: tokio::time::Instant) {
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
        while self.tasks.join_next().await.is_some() {}
    }
}

fn register_h264_codecs(media_engine: &mut MediaEngine) -> Result<(), PeerSessionError> {
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
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
    }
    Ok(())
}

fn claim_remote_video_track(
    already_claimed: &mut bool,
    mime_type: &str,
) -> Result<(), PeerSessionError> {
    if !mime_type.eq_ignore_ascii_case(MIME_TYPE_H264) {
        return Err(PeerSessionError::Protocol(format!(
            "unexpected remote video codec {mime_type}; H.264 is required"
        )));
    }
    if *already_claimed {
        return Err(PeerSessionError::Protocol(
            "a second remote video track is not allowed".into(),
        ));
    }
    *already_claimed = true;
    Ok(())
}

async fn send_data(
    channel: Option<&Arc<RTCDataChannel>>,
    data: String,
    name: &str,
    timeout: Duration,
) -> Result<(), PeerSessionError> {
    let channel = channel
        .ok_or_else(|| PeerSessionError::WebRtc(format!("{name} data channel is unavailable")))?;
    tokio::time::timeout(timeout, channel.send_text(data))
        .await
        // SCTP may have accepted the frame before the future times out. The
        // transport is closed by the actor, but callers must not treat this as
        // a definite non-delivery result.
        .map_err(|_| PeerSessionError::OutcomeUnknown)?
        .map(|_| ())
        .map_err(|error| PeerSessionError::WebRtc(error.to_string()))
}

async fn rtc_operation<T>(
    timeout: Duration,
    operation: impl std::future::Future<Output = webrtc::error::Result<T>>,
) -> Result<T, PeerSessionError> {
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| PeerSessionError::OperationTimeout)?
        .map_err(|error| PeerSessionError::WebRtc(error.to_string()))
}

async fn rtc_operation_until<T>(
    deadline: tokio::time::Instant,
    operation: impl std::future::Future<Output = webrtc::error::Result<T>>,
) -> Result<T, PeerSessionError> {
    tokio::time::timeout_at(deadline, operation)
        .await
        .map_err(|_| PeerSessionError::OperationTimeout)?
        .map_err(|error| PeerSessionError::WebRtc(error.to_string()))
}

fn validate_inbound_data_frame(
    kind: ChannelKind,
    is_string: bool,
    data: &[u8],
    max_bytes: usize,
) -> Result<(), String> {
    if !is_string {
        return Err(format!("{} data channel received an unexpected binary frame", kind.label()));
    }
    if std::str::from_utf8(data).is_err() {
        return Err(format!("{} data channel received invalid UTF-8", kind.label()));
    }
    if data.len() > max_bytes {
        return Err(format!(
            "{} data-channel frame exceeds the {} byte limit",
            kind.label(),
            max_bytes
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn early_ice_candidates_are_bounded_and_queued_until_remote_sdp() {
        let (event_tx, _event_rx) = mpsc::channel(8);
        let (fatal_tx, _fatal_rx) = watch::channel(None);
        let (video_tx, _) = broadcast::channel(2);
        let mut peer = RtcPeer::new(
            RtcConfig {
                ice_servers: Vec::new(),
                max_depacket_latency: Duration::from_millis(100),
                max_pending_candidates: 1,
                max_data_message_bytes: 16 * 1024,
                operation_timeout: Duration::from_secs(1),
            },
            event_tx,
            fatal_tx,
            video_tx,
        )
        .await
        .unwrap();
        let candidate = RTCIceCandidateInit {
            candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".into(),
            ..Default::default()
        };

        peer.add_remote_candidate(candidate.clone()).await.unwrap();
        assert_eq!(peer.pending_candidates, vec![candidate.clone()]);
        assert!(matches!(
            peer.add_remote_candidate(candidate).await,
            Err(PeerSessionError::Protocol(_))
        ));
        peer.shutdown().await;
    }

    #[test]
    fn rtc_event_queue_overflow_uses_the_nonblocking_fatal_path() {
        let (tx, _rx) = mpsc::channel(1);
        let (fatal_tx, fatal_rx) = watch::channel(None);
        let events = RtcEventDispatcher { tx, fatal_tx };
        events.dispatch(RtcEvent::PeerState(RTCPeerConnectionState::New));
        events.dispatch(RtcEvent::PeerState(RTCPeerConnectionState::Connecting));
        assert_eq!(fatal_rx.borrow().as_deref(), Some("WebRTC event queue overflowed"));
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
        let peer = RtcPeer::new(
            RtcConfig {
                ice_servers: Vec::new(),
                max_depacket_latency: Duration::from_millis(100),
                max_pending_candidates: 1,
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
}
