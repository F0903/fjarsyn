mod share_epoch;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use sha2::{Digest, Sha256};
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
        data_channel_message::DataChannelMessage, data_channel_state::RTCDataChannelState,
    },
    dtls_transport::dtls_transport_state::RTCDtlsTransportState,
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_connection_state::RTCIceConnectionState,
        ice_gathering_state::RTCIceGatheringState,
        ice_server::RTCIceServer,
        ice_transport_state::RTCIceTransportState,
    },
    interceptor::registry::Registry,
    media::{Sample, io::sample_builder::SampleBuilder},
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration, offer_answer_options::RTCOfferOptions,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, signaling_state::RTCSignalingState,
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

use super::{
    EncodedVideoSample, PeerSessionError, ShareEpoch,
    media::{OutboundVideoSample, RemoteVideoSample},
    restart::TransportGeneration,
};

pub(crate) const CONTROL_CHANNEL_LABEL: &str = "fjarsyn-control-v2";
pub(crate) const MESSAGING_CHANNEL_LABEL: &str = "fjarsyn-messaging-v2";

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
    LocalCandidate { generation: TransportGeneration, candidate: RTCIceCandidateInit },
    IceState { generation: TransportGeneration, state: RTCIceConnectionState },
    DtlsState { generation: TransportGeneration, state: RTCDtlsTransportState },
    PeerState { generation: TransportGeneration, state: RTCPeerConnectionState },
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
    pub max_candidates_per_generation: usize,
    pub max_data_message_bytes: usize,
    pub operation_timeout: Duration,
}

#[derive(Clone, PartialEq, Eq)]
struct IceCredentials {
    username_fragment: String,
    password_digest: [u8; 32],
}

impl IceCredentials {
    fn from_sdp(sdp: &str) -> Result<Self, PeerSessionError> {
        let mut username_fragment: Option<&str> = None;
        let mut password: Option<&str> = None;
        for line in sdp.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("a=ice-ufrag:") {
                record_unique_ice_attribute(&mut username_fragment, value, "username fragment")?;
            } else if let Some(value) = line.strip_prefix("a=ice-pwd:") {
                record_unique_ice_attribute(&mut password, value, "password")?;
            }
        }
        let username_fragment = username_fragment
            .filter(|value| !value.is_empty())
            .ok_or_else(|| PeerSessionError::Protocol("SDP has no ICE username fragment".into()))?;
        let password = password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| PeerSessionError::Protocol("SDP has no ICE password".into()))?;
        Ok(Self {
            username_fragment: username_fragment.to_owned(),
            password_digest: Sha256::digest(password.as_bytes()).into(),
        })
    }

    fn require_rotation_from(&self, previous: &Self, side: &str) -> Result<(), PeerSessionError> {
        if self.username_fragment == previous.username_fragment
            || self.password_digest == previous.password_digest
        {
            return Err(PeerSessionError::Protocol(format!(
                "ICE restart did not rotate both {side} credentials"
            )));
        }
        Ok(())
    }
}

fn record_unique_ice_attribute<'a>(
    slot: &mut Option<&'a str>,
    value: &'a str,
    name: &str,
) -> Result<(), PeerSessionError> {
    if let Some(existing) = slot
        && *existing != value
    {
        return Err(PeerSessionError::Protocol(format!("SDP contains multiple ICE {name}s")));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) struct RtcPeer {
    pc: Arc<RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticSample>,
    control: Option<Arc<RTCDataChannel>>,
    messaging: Option<Arc<RTCDataChannel>>,
    generation: TransportGeneration,
    callback_generation: Arc<AtomicU64>,
    local_description_generation: Option<TransportGeneration>,
    remote_description_generation: Option<TransportGeneration>,
    local_ice_credentials: Option<IceCredentials>,
    remote_ice_credentials: Option<IceCredentials>,
    share_epoch_extension_id: Option<u8>,
    max_candidates_per_generation: usize,
    remote_candidates_received: usize,
    max_data_message_bytes: usize,
    operation_timeout: Duration,
    max_depacket_latency: Duration,
    remote_video_tx: broadcast::Sender<RemoteVideoSample>,
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
        remote_video_tx: broadcast::Sender<RemoteVideoSample>,
    ) -> Result<Self, PeerSessionError> {
        let mut media_engine = MediaEngine::default();
        register_h264_codecs(&mut media_engine)?;
        share_epoch::register(&mut media_engine)?;
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

    fn register_peer_callbacks(
        pc: &Arc<RTCPeerConnection>,
        events: RtcEventDispatcher,
        callback_generation: Arc<AtomicU64>,
    ) {
        let candidate_events = events.clone();
        let candidate_generation = callback_generation.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let candidate_events = candidate_events.clone();
            let generation =
                TransportGeneration::from_value(candidate_generation.load(Ordering::Acquire));
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                match candidate.to_json() {
                    Ok(candidate) => candidate_events
                        .dispatch(RtcEvent::LocalCandidate { generation, candidate }),
                    Err(error) => candidate_events.dispatch(RtcEvent::Error(error.to_string())),
                }
            })
        }));

        let state_events = events.clone();
        let state_generation = callback_generation.clone();
        pc.on_peer_connection_state_change(Box::new(move |state| {
            let state_events = state_events.clone();
            let generation =
                TransportGeneration::from_value(state_generation.load(Ordering::Acquire));
            Box::pin(async move {
                state_events.dispatch(RtcEvent::PeerState { generation, state });
            })
        }));

        let ice_events = events.clone();
        let ice_generation = callback_generation.clone();
        pc.on_ice_connection_state_change(Box::new(move |state| {
            let ice_events = ice_events.clone();
            let generation =
                TransportGeneration::from_value(ice_generation.load(Ordering::Acquire));
            Box::pin(async move {
                ice_events.dispatch(RtcEvent::IceState { generation, state });
            })
        }));

        let dtls_events = events.clone();
        let dtls_generation = callback_generation.clone();
        pc.dtls_transport().on_state_change(Box::new(move |state| {
            let dtls_events = dtls_events.clone();
            let generation =
                TransportGeneration::from_value(dtls_generation.load(Ordering::Acquire));
            Box::pin(async move {
                dtls_events.dispatch(RtcEvent::DtlsState { generation, state });
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

    pub(crate) async fn create_offer(&mut self) -> Result<String, PeerSessionError> {
        let offer = rtc_operation(self.operation_timeout, self.pc.create_offer(None)).await?;
        let sdp = offer.sdp.clone();
        let credentials = IceCredentials::from_sdp(&sdp)?;
        self.set_local_description(TransportGeneration::INITIAL, offer, credentials).await?;
        Ok(sdp)
    }

    pub(crate) async fn apply_offer_and_create_answer(
        &mut self,
        sdp: String,
    ) -> Result<String, PeerSessionError> {
        let remote_credentials = IceCredentials::from_sdp(&sdp)?;
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(TransportGeneration::INITIAL, offer, remote_credentials)
            .await?;
        let answer = rtc_operation(self.operation_timeout, self.pc.create_answer(None)).await?;
        let answer_sdp = answer.sdp.clone();
        let local_credentials = IceCredentials::from_sdp(&answer_sdp)?;
        self.set_local_description(TransportGeneration::INITIAL, answer, local_credentials).await?;
        Ok(answer_sdp)
    }

    pub(crate) async fn apply_answer(&mut self, sdp: String) -> Result<(), PeerSessionError> {
        let credentials = IceCredentials::from_sdp(&sdp)?;
        let answer = RTCSessionDescription::answer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(TransportGeneration::INITIAL, answer, credentials).await
    }

    pub(crate) async fn create_restart_offer(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<String, PeerSessionError> {
        let previous_credentials = self.local_ice_credentials.clone().ok_or_else(|| {
            PeerSessionError::WebRtc("initial local ICE credentials are unavailable".into())
        })?;
        self.begin_restart_generation(generation).await?;
        let offer = rtc_operation(
            self.operation_timeout,
            self.pc.create_offer(Some(RTCOfferOptions { ice_restart: true, ..Default::default() })),
        )
        .await?;
        let sdp = offer.sdp.clone();
        let credentials = IceCredentials::from_sdp(&sdp)?;
        credentials.require_rotation_from(&previous_credentials, "local")?;
        self.set_local_description(generation, offer, credentials).await?;
        Ok(sdp)
    }

    pub(crate) async fn apply_restart_offer_and_create_answer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<String, PeerSessionError> {
        let remote_credentials = IceCredentials::from_sdp(&sdp)?;
        let previous_remote = self.remote_ice_credentials.clone().ok_or_else(|| {
            PeerSessionError::WebRtc("initial remote ICE credentials are unavailable".into())
        })?;
        remote_credentials.require_rotation_from(&previous_remote, "remote")?;
        let previous_local = self.local_ice_credentials.clone().ok_or_else(|| {
            PeerSessionError::WebRtc("initial local ICE credentials are unavailable".into())
        })?;
        self.begin_restart_generation(generation).await?;
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(generation, offer, remote_credentials).await?;
        let answer = rtc_operation(self.operation_timeout, self.pc.create_answer(None)).await?;
        let answer_sdp = answer.sdp.clone();
        let local_credentials = IceCredentials::from_sdp(&answer_sdp)?;
        local_credentials.require_rotation_from(&previous_local, "local")?;
        self.set_local_description(generation, answer, local_credentials).await?;
        Ok(answer_sdp)
    }

    pub(crate) async fn apply_restart_answer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<(), PeerSessionError> {
        self.require_generation(generation)?;
        let credentials = IceCredentials::from_sdp(&sdp)?;
        let previous_credentials = self.remote_ice_credentials.as_ref().ok_or_else(|| {
            PeerSessionError::WebRtc("initial remote ICE credentials are unavailable".into())
        })?;
        credentials.require_rotation_from(previous_credentials, "remote")?;
        let answer = RTCSessionDescription::answer(sdp)
            .map_err(|error| PeerSessionError::WebRtc(error.to_string()))?;
        self.set_remote_description(generation, answer, credentials).await
    }

    async fn begin_restart_generation(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), PeerSessionError> {
        if generation != self.generation.next()? {
            return Err(PeerSessionError::Protocol(
                "WebRTC restart did not use the next transport generation".into(),
            ));
        }
        if self.pc.signaling_state() != RTCSignalingState::Stable {
            return Err(PeerSessionError::WebRtc(
                "cannot restart ICE while WebRTC signaling is not stable".into(),
            ));
        }
        if self.pc.ice_gathering_state() == RTCIceGatheringState::Gathering {
            let mut gathering_complete = self.pc.gathering_complete_promise().await;
            tokio::time::timeout(self.operation_timeout, gathering_complete.recv())
                .await
                .map_err(|_| PeerSessionError::OperationTimeout)?
                .ok_or_else(|| {
                    PeerSessionError::WebRtc(
                        "ICE gathering completion notification was lost".into(),
                    )
                })?;
        }

        self.generation = generation;
        self.callback_generation.store(generation.value(), Ordering::Release);
        self.local_description_generation = None;
        self.remote_description_generation = None;
        self.remote_candidates_received = 0;
        Ok(())
    }

    async fn set_local_description(
        &mut self,
        generation: TransportGeneration,
        description: RTCSessionDescription,
        credentials: IceCredentials,
    ) -> Result<(), PeerSessionError> {
        self.require_generation(generation)?;
        let extension_id = self.validate_share_epoch_extension(&description.sdp)?;
        rtc_operation(self.operation_timeout, self.pc.set_local_description(description)).await?;
        self.share_epoch_extension_id = Some(extension_id);
        self.local_description_generation = Some(generation);
        self.local_ice_credentials = Some(credentials);
        Ok(())
    }

    async fn set_remote_description(
        &mut self,
        generation: TransportGeneration,
        description: RTCSessionDescription,
        credentials: IceCredentials,
    ) -> Result<(), PeerSessionError> {
        self.require_generation(generation)?;
        let extension_id = self.validate_share_epoch_extension(&description.sdp)?;
        rtc_operation(self.operation_timeout, self.pc.set_remote_description(description)).await?;
        self.share_epoch_extension_id = Some(extension_id);
        self.remote_description_generation = Some(generation);
        self.remote_ice_credentials = Some(credentials);
        Ok(())
    }

    fn validate_share_epoch_extension(&self, sdp: &str) -> Result<u8, PeerSessionError> {
        let extension_id = share_epoch::video_sdp_id(sdp)?;
        if self.share_epoch_extension_id.is_some_and(|negotiated| negotiated != extension_id) {
            return Err(PeerSessionError::Protocol(
                "screen-share epoch RTP extension ID changed within the session".into(),
            ));
        }
        Ok(extension_id)
    }

    pub(crate) fn prepare_local_candidate(
        &self,
        generation: TransportGeneration,
        mut candidate: RTCIceCandidateInit,
    ) -> Result<RTCIceCandidateInit, PeerSessionError> {
        self.require_generation(generation)?;
        if self.local_description_generation != Some(generation) {
            return Err(PeerSessionError::Protocol(
                "local ICE candidate arrived before its description was installed".into(),
            ));
        }
        let expected = &self
            .local_ice_credentials
            .as_ref()
            .ok_or_else(|| {
                PeerSessionError::WebRtc("local ICE credentials are unavailable".into())
            })?
            .username_fragment;
        if candidate.username_fragment.as_ref().is_some_and(|actual| actual != expected) {
            return Err(PeerSessionError::Protocol(
                "local ICE candidate used the wrong username fragment".into(),
            ));
        }
        candidate.username_fragment = Some(expected.clone());
        Ok(candidate)
    }

    pub(crate) async fn add_remote_candidate(
        &mut self,
        generation: TransportGeneration,
        candidate: RTCIceCandidateInit,
    ) -> Result<(), PeerSessionError> {
        self.require_generation(generation)?;
        if self.remote_description_generation != Some(generation) {
            return Err(PeerSessionError::Protocol(
                "ICE candidate arrived before its remote description".into(),
            ));
        }
        let expected = &self
            .remote_ice_credentials
            .as_ref()
            .ok_or_else(|| {
                PeerSessionError::WebRtc("remote ICE credentials are unavailable".into())
            })?
            .username_fragment;
        if candidate.username_fragment.as_deref() != Some(expected.as_str()) {
            return Err(PeerSessionError::Protocol(
                "remote ICE candidate used the wrong username fragment".into(),
            ));
        }
        if self.remote_candidates_received >= self.max_candidates_per_generation {
            return Err(PeerSessionError::Protocol(
                "too many ICE candidates for one transport generation".into(),
            ));
        }
        self.remote_candidates_received += 1;
        rtc_operation(self.operation_timeout, self.pc.add_ice_candidate(candidate)).await
    }

    pub(crate) fn ice_connection_state(&self) -> RTCIceConnectionState {
        self.pc.ice_connection_state()
    }

    pub(crate) fn peer_connection_state(&self) -> RTCPeerConnectionState {
        self.pc.connection_state()
    }

    pub(crate) fn dtls_state(&self) -> RTCDtlsTransportState {
        self.pc.dtls_transport().state()
    }

    pub(crate) async fn transport_is_operational(
        &self,
        generation: TransportGeneration,
    ) -> Result<bool, PeerSessionError> {
        self.require_generation(generation)?;
        if self.local_description_generation != Some(generation)
            || self.remote_description_generation != Some(generation)
            || !matches!(
                self.pc.ice_connection_state(),
                RTCIceConnectionState::Connected | RTCIceConnectionState::Completed
            )
            || self.pc.connection_state() != RTCPeerConnectionState::Connected
            || !matches!(
                self.pc.dtls_transport().ice_transport().state(),
                RTCIceTransportState::Connected | RTCIceTransportState::Completed
            )
            || self.pc.dtls_transport().state() != RTCDtlsTransportState::Connected
            || !self
                .control
                .as_ref()
                .is_some_and(|channel| channel.ready_state() == RTCDataChannelState::Open)
            || !self
                .messaging
                .as_ref()
                .is_some_and(|channel| channel.ready_state() == RTCDataChannelState::Open)
        {
            return Ok(false);
        }
        let pair = tokio::time::timeout(
            self.operation_timeout,
            self.pc.dtls_transport().ice_transport().get_selected_candidate_pair(),
        )
        .await
        .map_err(|_| PeerSessionError::OperationTimeout)?;
        Ok(pair.is_some())
    }

    fn require_generation(&self, generation: TransportGeneration) -> Result<(), PeerSessionError> {
        if generation != self.generation {
            return Err(PeerSessionError::Protocol(
                "WebRTC operation used the wrong transport generation".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn start_remote_track(
        &mut self,
        track: Arc<TrackRemote>,
        transceiver: Arc<RTCRtpTransceiver>,
    ) -> Result<(), PeerSessionError> {
        let codec = track.codec();
        claim_remote_video_track(&mut self.remote_video_claimed, &codec.capability.mime_type)?;
        let extension_id = share_epoch::negotiated_id(&track.params())?;
        if self.share_epoch_extension_id != Some(extension_id) {
            return Err(PeerSessionError::Protocol(
                "remote track used the wrong screen-share epoch RTP extension ID".into(),
            ));
        }
        let remote_video_tx = self.remote_video_tx.clone();
        let protocol_events = self.events.clone();
        let max_delay = self.max_depacket_latency;
        let media_ssrc = track.ssrc();
        self.tasks.spawn(async move {
            let clock_rate = codec.capability.clock_rate;
            let new_builder = || {
                SampleBuilder::new(
                    Self::SAMPLE_BUILDER_PACKET_WINDOW,
                    webrtc::rtp::codecs::h264::H264Packet::default(),
                    clock_rate,
                )
                .with_max_time_delay(max_delay)
            };
            let mut builder = new_builder();
            let mut active_epoch: Option<ShareEpoch> = None;

            loop {
                let (packet, _) = match track.read_rtp().await {
                    Ok(packet) => packet,
                    Err(error) => {
                        protocol_events.dispatch(RtcEvent::Error(format!(
                            "remote video RTP stream ended: {error}"
                        )));
                        return;
                    }
                };
                let epoch = match share_epoch::decode(&packet.header, extension_id) {
                    Ok(epoch) => epoch,
                    Err(error) => {
                        protocol_events.dispatch(RtcEvent::ProtocolError(error.to_string()));
                        return;
                    }
                };
                match share_epoch::classify_packet(active_epoch, epoch) {
                    share_epoch::PacketDisposition::DropStale => continue,
                    share_epoch::PacketDisposition::Advance => {
                        builder = new_builder();
                        active_epoch = Some(epoch);
                    }
                    share_epoch::PacketDisposition::Continue => {}
                }
                builder.push(packet);
                while let Some(sample) = builder.pop() {
                    let _ = remote_video_tx.send(RemoteVideoSample {
                        epoch: active_epoch.expect("an accepted RTP packet establishes an epoch"),
                        sample: EncodedVideoSample { data: sample.data, duration: sample.duration },
                    });
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
        tagged: OutboundVideoSample,
    ) -> Result<(), PeerSessionError> {
        let extension = share_epoch::outbound(tagged.epoch)?;
        let sample = Sample {
            data: tagged.sample.data,
            duration: tagged.sample.duration,
            ..Default::default()
        };
        tokio::time::timeout(
            self.operation_timeout,
            self.video_track.write_sample_with_extensions(&sample, &[extension]),
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
        let config = RtcConfig {
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            max_candidates_per_generation: 1,
            max_data_message_bytes: 16 * 1024,
            operation_timeout: Duration::from_secs(1),
        };
        let mut peer = RtcPeer::new(config.clone(), event_tx, fatal_tx, video_tx).await.unwrap();
        let candidate = RTCIceCandidateInit {
            candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".into(),
            ..Default::default()
        };

        assert!(matches!(
            peer.add_remote_candidate(TransportGeneration::INITIAL, candidate.clone()).await,
            Err(PeerSessionError::Protocol(_))
        ));
        let mut offerer =
            RtcPeer::new(config, offer_event_tx, offer_fatal_tx, offer_video_tx).await.unwrap();
        let offer = offerer.create_offer().await.unwrap();
        let remote_ufrag = IceCredentials::from_sdp(&offer).unwrap().username_fragment;
        peer.apply_offer_and_create_answer(offer).await.unwrap();
        let candidate = RTCIceCandidateInit { username_fragment: Some(remote_ufrag), ..candidate };
        peer.add_remote_candidate(TransportGeneration::INITIAL, candidate.clone()).await.unwrap();
        assert!(matches!(
            peer.add_remote_candidate(TransportGeneration::INITIAL, candidate).await,
            Err(PeerSessionError::Protocol(_))
        ));
        peer.shutdown().await;
        offerer.shutdown().await;
    }

    #[test]
    fn rtc_event_queue_overflow_uses_the_nonblocking_fatal_path() {
        let (tx, _rx) = mpsc::channel(1);
        let (fatal_tx, fatal_rx) = watch::channel(None);
        let events = RtcEventDispatcher { tx, fatal_tx };
        events.dispatch(RtcEvent::PeerState {
            generation: TransportGeneration::INITIAL,
            state: RTCPeerConnectionState::New,
        });
        events.dispatch(RtcEvent::PeerState {
            generation: TransportGeneration::INITIAL,
            state: RTCPeerConnectionState::Connecting,
        });
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
        let mut peer = RtcPeer::new(
            RtcConfig {
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
        let config = RtcConfig {
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            max_candidates_per_generation: 8,
            max_data_message_bytes: 16 * 1024,
            operation_timeout: Duration::from_secs(5),
        };
        let mut offerer =
            RtcPeer::new(config.clone(), offer_events, offer_fatal, offer_video).await.unwrap();
        let mut answerer =
            RtcPeer::new(config, answer_events, answer_fatal, answer_video).await.unwrap();

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
