use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    time::Instant,
};
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

use super::{
    EncodedVideoSink, LocalShareState, MessageId, PeerId, PeerSessionError, PeerSessionEvent,
    PeerSessionPhase, PeerSessionSnapshot, RemoteShareState, RemoteVideoSource, SessionCloseReason,
    SessionId, ShareId,
    media::{encoded_video_channel, remote_video_channel},
    negotiation::NegotiationConnection,
    protocol::{ControlMessage, DATA_PROTOCOL_VERSION, MessagingMessage, NegotiationSignal},
    rtc::{ChannelKind, RtcConfig, RtcEvent, RtcPeer},
    state_machine::{SessionInput, SessionStateMachine, SessionTransition},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionRole {
    Outgoing,
    Incoming,
}

pub(crate) struct SessionActorConfig {
    pub session_id: SessionId,
    pub remote_peer_id: PeerId,
    pub role: SessionRole,
    pub connection: Option<NegotiationConnection>,
    pub rtc: RtcConfig,
    pub command_capacity: usize,
    pub media_capacity: usize,
    pub remote_video_capacity: usize,
    pub max_message_bytes: usize,
    pub max_data_message_bytes: usize,
    pub request_timeout: std::time::Duration,
    pub negotiation_timeout: std::time::Duration,
    pub event_delivery_timeout: std::time::Duration,
    pub cleanup_timeout: std::time::Duration,
    pub pre_ready_data_capacity: usize,
    pub disconnected_grace: std::time::Duration,
    pub max_remote_timestamp_age: std::time::Duration,
    pub max_remote_clock_skew: std::time::Duration,
}

#[derive(Debug)]
pub(crate) enum SessionUpdate {
    Event { generation: uuid::Uuid, event: PeerSessionEvent },
}

#[derive(Debug)]
pub(crate) struct SessionTerminal {
    pub generation: uuid::Uuid,
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub reason: SessionCloseReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActorControl {
    Fail(String),
    TrustRevoked { deadline: Instant },
    Shutdown { deadline: Instant },
}

#[derive(Debug)]
pub(crate) enum SessionCommand {
    Accept(oneshot::Sender<Result<(), PeerSessionError>>),
    Reject {
        reason: String,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
    Disconnect(oneshot::Sender<Result<(), PeerSessionError>>),
    SendMessage {
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
    SendReceipt {
        message_id: MessageId,
        received_at: DateTime<Utc>,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
    StartShare(oneshot::Sender<Result<ShareId, PeerSessionError>>),
    StopShare {
        share_id: ShareId,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
}

impl SessionCommand {
    pub(crate) fn reply_error(self, error: PeerSessionError) {
        match self {
            Self::Accept(reply) | Self::Disconnect(reply) => {
                let _ = reply.send(Err(error));
            }
            Self::Reject { reply, .. }
            | Self::SendMessage { reply, .. }
            | Self::SendReceipt { reply, .. }
            | Self::StopShare { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::StartShare(reply) => {
                let _ = reply.send(Err(error));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionActorHandle {
    pub session_id: SessionId,
    pub generation: uuid::Uuid,
    command_tx: mpsc::Sender<SessionCommand>,
    snapshot_rx: watch::Receiver<PeerSessionSnapshot>,
    encoded_video_sink: EncodedVideoSink,
    remote_video_tx: broadcast::Sender<super::EncodedVideoSample>,
    initial_remote_video_rx: Arc<Mutex<Option<broadcast::Receiver<super::EncodedVideoSample>>>>,
    fatal_tx: watch::Sender<Option<ActorControl>>,
}

impl SessionActorHandle {
    pub(crate) fn command_tx(&self) -> mpsc::Sender<SessionCommand> {
        self.command_tx.clone()
    }

    pub(crate) fn snapshot(&self) -> PeerSessionSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    pub(crate) fn encoded_video_sink(&self) -> EncodedVideoSink {
        self.encoded_video_sink.clone()
    }

    pub(crate) fn remote_video_source(&self) -> RemoteVideoSource {
        let receiver = self
            .initial_remote_video_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .unwrap_or_else(|| self.remote_video_tx.subscribe());
        RemoteVideoSource::new(self.session_id, receiver)
    }

    pub(crate) fn fail(&self, reason: impl Into<String>) {
        self.fatal_tx.send_replace(Some(ActorControl::Fail(reason.into())));
    }

    pub(crate) fn shutdown(&self, deadline: Instant) {
        self.fatal_tx.send_replace(Some(ActorControl::Shutdown { deadline }));
    }

    pub(crate) fn revoke_trust(&self, deadline: Instant) {
        self.fatal_tx.send_replace(Some(ActorControl::TrustRevoked { deadline }));
    }
}

pub(crate) fn spawn(
    mut config: SessionActorConfig,
    update_tx: mpsc::Sender<SessionUpdate>,
    terminal_tx: mpsc::UnboundedSender<SessionTerminal>,
) -> (SessionActorHandle, tokio::task::JoinHandle<()>) {
    let state = match config.role {
        SessionRole::Outgoing => SessionStateMachine::outgoing(),
        SessionRole::Incoming => SessionStateMachine::incoming(),
    };
    let generation = uuid::Uuid::new_v4();
    let snapshot = PeerSessionSnapshot {
        session_id: config.session_id,
        peer_id: config.remote_peer_id.clone(),
        phase: state.phase(),
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    };
    let (command_tx, command_rx) = mpsc::channel(config.command_capacity.max(1));
    let (snapshot_tx, snapshot_rx) = watch::channel(snapshot);
    let (video_sink, video_rx) = encoded_video_channel(config.session_id, config.media_capacity);
    let (remote_video_tx, initial_remote_video_rx) =
        remote_video_channel(config.remote_video_capacity);
    let (rtc_event_tx, rtc_event_rx) = mpsc::channel(config.command_capacity.max(8));
    let (rtc_fatal_tx, rtc_fatal_rx) = watch::channel(None);
    let (fatal_tx, fatal_rx) = watch::channel(None);

    let handle = SessionActorHandle {
        session_id: config.session_id,
        generation,
        command_tx,
        snapshot_rx,
        encoded_video_sink: video_sink,
        remote_video_tx: remote_video_tx.clone(),
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };
    let connection = config.connection.take().expect("session actor requires signaling");
    let actor = SessionActor {
        config,
        state,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
        connection: Some(connection),
        rtc: None,
        command_rx,
        video_rx,
        remote_video_tx,
        rtc_event_tx,
        rtc_event_rx,
        rtc_fatal_tx,
        rtc_fatal_rx,
        fatal_rx,
        snapshot_tx,
        update_tx,
        terminal_tx,
        pc_connected: false,
        control_open: false,
        messaging_open: false,
        terminal_reason: None,
        generation,
        phase_started: Instant::now(),
        local_transport_ready: false,
        remote_ready: false,
        ready_acknowledged: false,
        disconnected_since: None,
        cleanup_deadline: None,
        pending_application_data: VecDeque::new(),
    };
    let task = tokio::spawn(actor.run());
    (handle, task)
}

struct SessionActor {
    config: SessionActorConfig,
    state: SessionStateMachine,
    local_share: LocalShareState,
    remote_share: RemoteShareState,
    connection: Option<NegotiationConnection>,
    rtc: Option<RtcPeer>,
    command_rx: mpsc::Receiver<SessionCommand>,
    video_rx: mpsc::Receiver<super::EncodedVideoSample>,
    remote_video_tx: broadcast::Sender<super::EncodedVideoSample>,
    rtc_event_tx: mpsc::Sender<RtcEvent>,
    rtc_event_rx: mpsc::Receiver<RtcEvent>,
    rtc_fatal_tx: watch::Sender<Option<String>>,
    rtc_fatal_rx: watch::Receiver<Option<String>>,
    fatal_rx: watch::Receiver<Option<ActorControl>>,
    snapshot_tx: watch::Sender<PeerSessionSnapshot>,
    update_tx: mpsc::Sender<SessionUpdate>,
    terminal_tx: mpsc::UnboundedSender<SessionTerminal>,
    pc_connected: bool,
    control_open: bool,
    messaging_open: bool,
    terminal_reason: Option<SessionCloseReason>,
    generation: uuid::Uuid,
    phase_started: Instant,
    local_transport_ready: bool,
    remote_ready: bool,
    ready_acknowledged: bool,
    disconnected_since: Option<Instant>,
    cleanup_deadline: Option<Instant>,
    pending_application_data: VecDeque<(ChannelKind, bytes::Bytes)>,
}

impl SessionActor {
    async fn run(mut self) {
        let _ = self.publish_snapshot().await;
        let mut timeout_check = tokio::time::interval(std::time::Duration::from_millis(250));

        if self.config.role == SessionRole::Outgoing
            && let Err(error) = self.send_signal(NegotiationSignal::Request {}).await
        {
            self.terminal_reason =
                Some(SessionCloseReason::ConnectionFailed { reason: error.to_string() });
        }

        while self.terminal_reason.is_none() {
            tokio::select! {
                biased;
                changed = self.fatal_rx.changed() => {
                    if changed.is_err() {
                        self.fail("peer-session fatal control channel closed".into());
                    } else {
                        let control = self.fatal_rx.borrow_and_update().clone();
                        match control {
                            Some(ActorControl::Fail(reason)) => self.fail(reason),
                            Some(ActorControl::TrustRevoked { deadline }) => {
                                self.cleanup_deadline = Some(deadline);
                                self.terminal_reason = Some(SessionCloseReason::TrustRevoked);
                            }
                            Some(ActorControl::Shutdown { deadline }) => {
                                self.cleanup_deadline = Some(deadline);
                                self.terminal_reason = Some(SessionCloseReason::ServiceShutdown);
                            }
                            None => {}
                        }
                    }
                }
                changed = self.rtc_fatal_rx.changed(), if self.rtc.is_some() => {
                    if changed.is_err() {
                        self.fail("WebRTC fatal event channel closed".into());
                    } else {
                        let reason = self.rtc_fatal_rx.borrow_and_update().clone();
                        if let Some(reason) = reason {
                            self.fail(reason);
                        }
                    }
                }
                command = self.command_rx.recv() => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => self.terminal_reason = Some(SessionCloseReason::ServiceShutdown),
                    }
                }
                signal = async {
                    match self.connection.as_mut() {
                        Some(connection) => connection.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match signal {
                        Some(Ok(signal)) => self.handle_signal(signal).await,
                        Some(Err(error)) => self.fail_error(error),
                        None if self.state.phase() != PeerSessionPhase::Connected => {
                            self.terminal_reason = Some(SessionCloseReason::SignalingLost);
                        }
                        None => {}
                    }
                }
                event = self.rtc_event_rx.recv(), if self.rtc.is_some() => {
                    match event {
                        Some(event) => self.handle_rtc_event(event).await,
                        None => self.fail("WebRTC event channel closed".into()),
                    }
                }
                sample = self.video_rx.recv() => {
                    let Some(sample) = sample else { continue };
                    if matches!(self.local_share, LocalShareState::Active { .. })
                        && let Some(rtc) = self.rtc.as_ref()
                        && let Err(error) = rtc.write_video(sample).await
                    {
                        self.fail(error.to_string());
                    }
                }
                _ = timeout_check.tick() => {
                    let timeout = match self.state.phase() {
                        PeerSessionPhase::Requesting | PeerSessionPhase::Incoming => {
                            Some(self.config.request_timeout)
                        }
                        PeerSessionPhase::Negotiating => Some(self.config.negotiation_timeout),
                        PeerSessionPhase::Connected | PeerSessionPhase::Disconnecting => None,
                    };
                    if timeout.is_some_and(|timeout| self.phase_started.elapsed() >= timeout) {
                        self.fail(format!("{} phase timed out", self.state.phase().name()));
                    }
                    if self.state.phase() == PeerSessionPhase::Connected
                        && self.disconnected_since.is_some_and(|since| {
                            disconnect_grace_expired(
                                since,
                                Instant::now(),
                                self.config.disconnected_grace,
                            )
                        })
                    {
                        self.fail("peer connection did not recover from disconnection".into());
                    }
                }
            }
        }

        reject_queued_session_commands(&mut self.command_rx);

        let cleanup_deadline =
            self.cleanup_deadline.unwrap_or_else(|| Instant::now() + self.config.cleanup_timeout);
        let connection = self.connection.take();
        let rtc = self.rtc.take();
        tokio::join!(
            async move {
                if let Some(connection) = connection {
                    connection.shutdown_until(cleanup_deadline).await;
                }
            },
            async move {
                if let Some(rtc) = rtc {
                    rtc.shutdown_until(cleanup_deadline).await;
                }
            },
        );
        let reason = self.terminal_reason.unwrap_or(SessionCloseReason::ServiceShutdown);
        let _ = self.terminal_tx.send(SessionTerminal {
            generation: self.generation,
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            reason,
        });
    }

    async fn handle_command(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::Accept(reply) => {
                let result = self.accept().await;
                if let Err(error) = &result
                    && !matches!(error, PeerSessionError::InvalidState { .. })
                {
                    self.fail_error(error.clone());
                }
                let _ = reply.send(result);
            }
            SessionCommand::Reject { reason, reply } => {
                let result = self.reject(reason).await;
                let _ = reply.send(result);
            }
            SessionCommand::Disconnect(reply) => {
                let result = self.disconnect().await;
                let _ = reply.send(result);
            }
            SessionCommand::SendMessage { message_id, body, sent_at, reply } => {
                let result = self.send_message(message_id, body, sent_at).await;
                self.fail_on_message_command_error(&result);
                let _ = reply.send(result);
            }
            SessionCommand::SendReceipt { message_id, received_at, reply } => {
                let result = self.send_receipt(message_id, received_at).await;
                self.fail_on_message_command_error(&result);
                let _ = reply.send(result);
            }
            SessionCommand::StartShare(reply) => {
                let result = self.start_share().await;
                self.fail_on_terminal_command_error(&result);
                let _ = reply.send(result);
            }
            SessionCommand::StopShare { share_id, reply } => {
                let result = self.stop_share(share_id).await;
                self.fail_on_terminal_command_error(&result);
                let _ = reply.send(result);
            }
        }
    }

    async fn accept(&mut self) -> Result<(), PeerSessionError> {
        self.ensure_rtc().await?;
        self.apply(SessionInput::AcceptLocal).await?;
        self.send_signal(NegotiationSignal::Accept {}).await
    }

    async fn reject(&mut self, reason: String) -> Result<(), PeerSessionError> {
        if reason.len() > 512 {
            return Err(PeerSessionError::Protocol("rejection reason exceeds limit".into()));
        }
        self.apply(SessionInput::RejectLocal(reason.clone())).await?;
        self.send_signal(NegotiationSignal::Reject { reason: reason.clone() }).await?;
        self.terminal_reason = Some(SessionCloseReason::Rejected { reason });
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), PeerSessionError> {
        self.apply(SessionInput::DisconnectLocal).await?;
        if self.control_open {
            if let Some(rtc) = self.rtc.as_ref() {
                let payload = serde_json::to_string(&ControlMessage::Disconnect {
                    version: DATA_PROTOCOL_VERSION,
                })
                .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
                let _ = rtc.send_control(payload).await;
            }
        } else {
            let _ = self.send_signal(NegotiationSignal::Cancel {}).await;
        }
        self.terminal_reason = Some(SessionCloseReason::LocalDisconnect);
        Ok(())
    }

    async fn send_message(
        &mut self,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        self.require_connected("send a message")?;
        let message = MessagingMessage::Chat {
            version: DATA_PROTOCOL_VERSION,
            message_id,
            body: body.clone(),
            sent_at,
        };
        message.validate(self.config.max_message_bytes)?;
        let encoded = serde_json::to_string(&message)
            .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
        if encoded.len() > self.config.max_data_message_bytes {
            return Err(PeerSessionError::MessageTooLarge { max: self.config.max_message_bytes });
        }
        self.rtc
            .as_ref()
            .ok_or_else(|| PeerSessionError::WebRtc("peer connection is unavailable".into()))?
            .send_message(encoded)
            .await?;
        self.emit(PeerSessionEvent::MessageSent {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            message_id,
            body,
            sent_at,
        })
        .await;
        Ok(())
    }

    async fn send_receipt(
        &mut self,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        self.require_connected("send a receipt")?;
        let message =
            MessagingMessage::Receipt { version: DATA_PROTOCOL_VERSION, message_id, received_at };
        let encoded = serde_json::to_string(&message)
            .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
        self.rtc
            .as_ref()
            .ok_or_else(|| PeerSessionError::WebRtc("peer connection is unavailable".into()))?
            .send_message(encoded)
            .await
    }

    async fn start_share(&mut self) -> Result<ShareId, PeerSessionError> {
        self.require_connected("start screen sharing")?;
        if !matches!(self.local_share, LocalShareState::Inactive) {
            return Err(self.invalid_state("start another screen share"));
        }
        let share_id = ShareId::new();
        self.send_control(ControlMessage::ShareStarted {
            version: DATA_PROTOCOL_VERSION,
            share_id,
        })
        .await?;
        self.local_share = LocalShareState::Active { share_id };
        self.publish_snapshot().await;
        self.emit(PeerSessionEvent::LocalShareChanged {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            state: self.local_share,
        })
        .await;
        Ok(share_id)
    }

    async fn stop_share(&mut self, share_id: ShareId) -> Result<(), PeerSessionError> {
        self.require_connected("stop screen sharing")?;
        if self.local_share != (LocalShareState::Active { share_id }) {
            return Err(PeerSessionError::ShareMismatch(share_id));
        }
        self.send_control(ControlMessage::ShareStopped {
            version: DATA_PROTOCOL_VERSION,
            share_id,
        })
        .await?;
        self.local_share = LocalShareState::Inactive;
        self.publish_snapshot().await;
        self.emit(PeerSessionEvent::LocalShareChanged {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            state: self.local_share,
        })
        .await;
        Ok(())
    }

    async fn send_control(&self, message: ControlMessage) -> Result<(), PeerSessionError> {
        let encoded = serde_json::to_string(&message)
            .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
        self.rtc
            .as_ref()
            .ok_or_else(|| PeerSessionError::WebRtc("peer connection is unavailable".into()))?
            .send_control(encoded)
            .await
    }

    async fn handle_signal(&mut self, signal: NegotiationSignal) {
        let result = match signal {
            NegotiationSignal::EndpointHello { .. } | NegotiationSignal::EndpointProof { .. } => {
                Err(PeerSessionError::Protocol(
                    "endpoint-authentication message received after signaling authentication"
                        .into(),
                ))
            }
            NegotiationSignal::Request {} => {
                Err(PeerSessionError::Protocol("duplicate connection request".into()))
            }
            NegotiationSignal::Accept {} => self.handle_remote_accept().await,
            NegotiationSignal::Offer { sdp } => self.handle_offer(sdp).await,
            NegotiationSignal::Answer { sdp } => self.handle_answer(sdp).await,
            NegotiationSignal::IceCandidate { candidate } => match self.rtc.as_mut() {
                Some(rtc) => rtc.add_remote_candidate(candidate).await,
                None => Err(PeerSessionError::Protocol(
                    "ICE candidate arrived before acceptance".into(),
                )),
            },
            NegotiationSignal::Ready {} => self.handle_remote_ready().await,
            NegotiationSignal::ReadyAck {} => self.handle_ready_ack().await,
            NegotiationSignal::Reject { reason } => {
                if reason.len() > 512 {
                    return self.fail_error(PeerSessionError::Protocol(
                        "rejection reason exceeds limit".into(),
                    ));
                }
                match self.apply(SessionInput::RejectRemote(reason.clone())).await {
                    Ok(()) => self.terminal_reason = Some(SessionCloseReason::Rejected { reason }),
                    Err(error) => return self.fail(error.to_string()),
                }
                Ok(())
            }
            NegotiationSignal::Cancel {} => {
                match self.apply(SessionInput::Cancel).await {
                    Ok(()) => self.terminal_reason = Some(SessionCloseReason::Cancelled),
                    Err(error) => return self.fail(error.to_string()),
                }
                Ok(())
            }
        };
        if let Err(error) = result {
            self.fail_error(error);
        }
    }

    async fn handle_remote_accept(&mut self) -> Result<(), PeerSessionError> {
        self.apply(SessionInput::AcceptRemote).await?;
        self.ensure_rtc().await?;
        let rtc = self.rtc.as_mut().expect("RTC initialized above");
        rtc.prepare_offerer_channels().await?;
        let sdp = rtc.create_offer().await?;
        self.send_signal(NegotiationSignal::Offer { sdp }).await
    }

    async fn handle_offer(&mut self, sdp: String) -> Result<(), PeerSessionError> {
        if self.config.role != SessionRole::Incoming
            || self.state.phase() != PeerSessionPhase::Negotiating
        {
            return Err(self.invalid_state("apply a remote offer"));
        }
        let answer = self
            .rtc
            .as_mut()
            .ok_or_else(|| PeerSessionError::WebRtc("peer connection is unavailable".into()))?
            .apply_offer_and_create_answer(sdp)
            .await?;
        self.send_signal(NegotiationSignal::Answer { sdp: answer }).await
    }

    async fn handle_answer(&mut self, sdp: String) -> Result<(), PeerSessionError> {
        if self.config.role != SessionRole::Outgoing
            || self.state.phase() != PeerSessionPhase::Negotiating
        {
            return Err(self.invalid_state("apply a remote answer"));
        }
        self.rtc
            .as_mut()
            .ok_or_else(|| PeerSessionError::WebRtc("peer connection is unavailable".into()))?
            .apply_answer(sdp)
            .await
    }

    async fn handle_rtc_event(&mut self, event: RtcEvent) {
        let result = match event {
            RtcEvent::LocalCandidate(candidate) => {
                if should_forward_local_candidate(self.state.phase(), self.connection.is_some()) {
                    self.send_signal(NegotiationSignal::IceCandidate { candidate }).await
                } else {
                    // Trickle ICE callbacks may arrive after the readiness handshake has
                    // deliberately closed signaling. They cannot affect an established session.
                    Ok(())
                }
            }
            RtcEvent::PeerState(state) => match state {
                RTCPeerConnectionState::Connected => {
                    self.pc_connected = true;
                    self.disconnected_since = None;
                    self.try_announce_ready().await
                }
                RTCPeerConnectionState::Disconnected => {
                    self.pc_connected = false;
                    if self.state.phase() == PeerSessionPhase::Negotiating {
                        self.fail("peer connection disconnected during negotiation".into());
                    } else if self.state.phase() == PeerSessionPhase::Connected {
                        self.disconnected_since.get_or_insert_with(Instant::now);
                    }
                    Ok(())
                }
                RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                    self.fail(format!("peer connection became {state}"));
                    Ok(())
                }
                _ => Ok(()),
            },
            RtcEvent::DataChannel(channel) => match self.rtc.as_mut() {
                Some(rtc) => rtc.attach_data_channel(channel),
                None => Err(PeerSessionError::WebRtc("peer connection is unavailable".into())),
            },
            RtcEvent::ChannelOpen(kind) => {
                match kind {
                    ChannelKind::Control => self.control_open = true,
                    ChannelKind::Messaging => self.messaging_open = true,
                }
                self.try_announce_ready().await
            }
            RtcEvent::ChannelClosed(kind) => {
                match kind {
                    ChannelKind::Control => self.control_open = false,
                    ChannelKind::Messaging => self.messaging_open = false,
                }
                if channel_close_is_terminal(self.state.phase()) {
                    self.fail(format!(
                        "{} data channel closed",
                        match kind {
                            ChannelKind::Control => "control",
                            ChannelKind::Messaging => "messaging",
                        }
                    ));
                }
                Ok(())
            }
            RtcEvent::ChannelMessage(kind, data) => self.route_channel_message(kind, data).await,
            RtcEvent::RemoteTrack(track, transceiver) => match self.rtc.as_mut() {
                Some(rtc) => rtc.start_remote_track(track, transceiver),
                None => Err(PeerSessionError::WebRtc("peer connection is unavailable".into())),
            },
            RtcEvent::Error(reason) => Err(PeerSessionError::WebRtc(reason)),
            RtcEvent::ProtocolError(reason) => Err(PeerSessionError::Protocol(reason)),
        };
        if let Err(error) = result {
            self.fail_error(error);
        }
    }

    async fn route_channel_message(
        &mut self,
        kind: ChannelKind,
        data: bytes::Bytes,
    ) -> Result<(), PeerSessionError> {
        let is_disconnect = if self.state.phase() == PeerSessionPhase::Negotiating
            && kind == ChannelKind::Control
        {
            let message: ControlMessage = serde_json::from_slice(&data)
                .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
            message.validate()?;
            matches!(message, ControlMessage::Disconnect { .. })
        } else {
            false
        };

        match application_data_disposition(
            self.state.phase(),
            is_disconnect,
            self.local_transport_ready,
            self.remote_ready,
        ) {
            ApplicationDataDisposition::Deliver => self.handle_channel_message(kind, data).await,
            ApplicationDataDisposition::Buffer => {
                // A data-channel frame is already protected by DTLS. It can race the
                // final ReadyAck on the separate signaling transport, so retain a
                // small ordered window until this actor commits Connected.
                if self.pending_application_data.len() >= self.config.pre_ready_data_capacity {
                    return Err(PeerSessionError::Protocol(
                        "too many application frames arrived before readiness".into(),
                    ));
                }
                validate_buffered_application_data(kind, &data, self.config.max_message_bytes)?;
                self.pending_application_data.push_back((kind, data));
                Ok(())
            }
            ApplicationDataDisposition::Reject => Err(PeerSessionError::Protocol(
                "application data arrived outside an active session".into(),
            )),
        }
    }

    async fn flush_pending_application_data(&mut self) -> Result<(), PeerSessionError> {
        while let Some((kind, data)) = self.pending_application_data.pop_front() {
            self.handle_channel_message(kind, data).await?;
        }
        Ok(())
    }

    async fn handle_channel_message(
        &mut self,
        kind: ChannelKind,
        data: bytes::Bytes,
    ) -> Result<(), PeerSessionError> {
        if data.len() > self.config.max_data_message_bytes {
            return Err(PeerSessionError::Protocol("data-channel message exceeds limit".into()));
        }
        match kind {
            ChannelKind::Control => {
                let message: ControlMessage = serde_json::from_slice(&data)
                    .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
                message.validate()?;
                match message {
                    ControlMessage::ShareStarted { share_id, .. } => {
                        require_connected_application_data(self.state.phase())?;
                        if !matches!(self.remote_share, RemoteShareState::Inactive) {
                            return Err(PeerSessionError::Protocol(
                                "remote started a second screen share".into(),
                            ));
                        }
                        self.remote_share = RemoteShareState::Active { share_id };
                        self.publish_snapshot().await;
                        self.emit(PeerSessionEvent::RemoteShareChanged {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            state: self.remote_share,
                        })
                        .await;
                    }
                    ControlMessage::ShareStopped { share_id, .. } => {
                        require_connected_application_data(self.state.phase())?;
                        if self.remote_share != (RemoteShareState::Active { share_id }) {
                            return Err(PeerSessionError::ShareMismatch(share_id));
                        }
                        self.remote_share = RemoteShareState::Inactive;
                        self.publish_snapshot().await;
                        self.emit(PeerSessionEvent::RemoteShareChanged {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            state: self.remote_share,
                        })
                        .await;
                    }
                    ControlMessage::Disconnect { .. } => {
                        let _ = self.apply(SessionInput::DisconnectRemote).await;
                        self.terminal_reason = Some(SessionCloseReason::RemoteDisconnect);
                    }
                }
            }
            ChannelKind::Messaging => {
                require_connected_application_data(self.state.phase())?;
                let message: MessagingMessage = serde_json::from_slice(&data)
                    .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
                message.validate(self.config.max_message_bytes)?;
                match message {
                    MessagingMessage::Chat { message_id, body, sent_at, .. } => {
                        validate_remote_timestamp(
                            sent_at,
                            Utc::now(),
                            self.config.max_remote_timestamp_age,
                            self.config.max_remote_clock_skew,
                        )?;
                        self.emit(PeerSessionEvent::MessageReceived {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            message_id,
                            body,
                            sent_at,
                        })
                        .await;
                    }
                    MessagingMessage::Receipt { message_id, received_at, .. } => {
                        validate_remote_timestamp(
                            received_at,
                            Utc::now(),
                            self.config.max_remote_timestamp_age,
                            self.config.max_remote_clock_skew,
                        )?;
                        self.emit(PeerSessionEvent::MessageReceiptReceived {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            message_id,
                            received_at,
                        })
                        .await;
                    }
                }
            }
        }
        Ok(())
    }

    async fn try_announce_ready(&mut self) -> Result<(), PeerSessionError> {
        if self.state.phase() == PeerSessionPhase::Negotiating
            && self.pc_connected
            && self.control_open
            && self.messaging_open
            && !self.local_transport_ready
        {
            self.local_transport_ready = true;
            self.send_signal(NegotiationSignal::Ready {}).await?;
            self.try_finish_ready().await?;
        }
        Ok(())
    }

    async fn handle_remote_ready(&mut self) -> Result<(), PeerSessionError> {
        if self.state.phase() != PeerSessionPhase::Negotiating || self.remote_ready {
            return Err(PeerSessionError::Protocol("unexpected session-ready signal".into()));
        }
        self.remote_ready = true;
        // `NegotiationConnection::send` resolves only after the frame is written,
        // so closing signaling after this point cannot discard the acknowledgement.
        self.send_signal(NegotiationSignal::ReadyAck {}).await?;
        self.try_finish_ready().await
    }

    async fn handle_ready_ack(&mut self) -> Result<(), PeerSessionError> {
        if self.state.phase() != PeerSessionPhase::Negotiating
            || !self.local_transport_ready
            || self.ready_acknowledged
        {
            return Err(PeerSessionError::Protocol(
                "unexpected session-ready acknowledgement".into(),
            ));
        }
        self.ready_acknowledged = true;
        self.try_finish_ready().await
    }

    async fn try_finish_ready(&mut self) -> Result<(), PeerSessionError> {
        if self.state.phase() == PeerSessionPhase::Negotiating
            && self.local_transport_ready
            && self.remote_ready
            && self.ready_acknowledged
        {
            self.apply(SessionInput::TransportReady).await?;
            if let Some(connection) = self.connection.take() {
                connection.shutdown().await;
            }
            self.emit(PeerSessionEvent::Connected {
                session_id: self.config.session_id,
                peer_id: self.config.remote_peer_id.clone(),
            })
            .await;
            self.flush_pending_application_data().await?;
        }
        Ok(())
    }

    async fn ensure_rtc(&mut self) -> Result<(), PeerSessionError> {
        if self.rtc.is_none() {
            self.rtc = Some(
                RtcPeer::new(
                    self.config.rtc.clone(),
                    self.rtc_event_tx.clone(),
                    self.rtc_fatal_tx.clone(),
                    self.remote_video_tx.clone(),
                )
                .await?,
            );
        }
        Ok(())
    }

    async fn send_signal(&self, signal: NegotiationSignal) -> Result<(), PeerSessionError> {
        self.connection
            .as_ref()
            .ok_or_else(|| PeerSessionError::Signaling("signaling connection is closed".into()))?
            .send(signal)
            .await
    }

    async fn apply(&mut self, input: SessionInput) -> Result<(), PeerSessionError> {
        let transition =
            self.state.apply(input).map_err(|error| PeerSessionError::InvalidState {
                session_id: self.config.session_id,
                phase: error.phase.name(),
                operation: "apply session transition",
            })?;
        match transition {
            SessionTransition::Phase(_) => {
                self.phase_started = Instant::now();
                self.publish_snapshot().await;
            }
            SessionTransition::Close(reason) => self.terminal_reason = Some(reason),
        }
        Ok(())
    }

    fn require_connected(&self, operation: &'static str) -> Result<(), PeerSessionError> {
        if self.state.phase() == PeerSessionPhase::Connected {
            Ok(())
        } else {
            Err(PeerSessionError::InvalidState {
                session_id: self.config.session_id,
                phase: self.state.phase().name(),
                operation,
            })
        }
    }

    fn invalid_state(&self, operation: &'static str) -> PeerSessionError {
        PeerSessionError::InvalidState {
            session_id: self.config.session_id,
            phase: self.state.phase().name(),
            operation,
        }
    }

    fn fail(&mut self, reason: String) {
        let _ = self.state.apply(SessionInput::Fail(reason.clone()));
        self.terminal_reason = Some(SessionCloseReason::ConnectionFailed { reason });
    }

    fn fail_error(&mut self, error: PeerSessionError) {
        let reason = error.to_string();
        let _ = self.state.apply(SessionInput::Fail(reason.clone()));
        self.terminal_reason = Some(match error {
            PeerSessionError::Protocol(_) | PeerSessionError::ShareMismatch(_) => {
                SessionCloseReason::ProtocolViolation { reason }
            }
            _ => SessionCloseReason::ConnectionFailed { reason },
        });
    }

    fn fail_on_terminal_command_error<T>(&mut self, result: &Result<T, PeerSessionError>) {
        if let Err(error) = result
            && command_error_is_terminal(error, true)
        {
            self.fail_error(error.clone());
        }
    }

    fn fail_on_message_command_error<T>(&mut self, result: &Result<T, PeerSessionError>) {
        if let Err(error) = result
            && command_error_is_terminal(error, false)
        {
            self.fail_error(error.clone());
        }
    }

    async fn publish_snapshot(&self) {
        let snapshot = PeerSessionSnapshot {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            phase: self.state.phase(),
            local_share: self.local_share,
            remote_share: self.remote_share,
        };
        self.snapshot_tx.send_replace(snapshot.clone());
        // The actor handle's watch channel is the coalescing source of truth.
        // The service periodically projects all actor watches into its snapshot.
    }

    async fn emit(&mut self, event: PeerSessionEvent) {
        let update = SessionUpdate::Event { generation: self.generation, event };
        match tokio::time::timeout(self.config.event_delivery_timeout, self.update_tx.send(update))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => self.fail("peer-session event receiver closed".into()),
            Err(_) => self.fail("peer-session event delivery timed out".into()),
        }
    }
}

fn disconnect_grace_expired(since: Instant, now: Instant, grace: std::time::Duration) -> bool {
    now.duration_since(since) >= grace
}

fn reject_queued_session_commands(command_rx: &mut mpsc::Receiver<SessionCommand>) {
    command_rx.close();
    while let Ok(command) = command_rx.try_recv() {
        command.reply_error(PeerSessionError::ServiceStopped);
    }
}

fn channel_close_is_terminal(phase: PeerSessionPhase) -> bool {
    matches!(phase, PeerSessionPhase::Negotiating | PeerSessionPhase::Connected)
}

fn should_forward_local_candidate(phase: PeerSessionPhase, signaling_open: bool) -> bool {
    phase == PeerSessionPhase::Negotiating && signaling_open
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplicationDataDisposition {
    Deliver,
    Buffer,
    Reject,
}

fn application_data_disposition(
    phase: PeerSessionPhase,
    is_disconnect: bool,
    local_transport_ready: bool,
    remote_ready: bool,
) -> ApplicationDataDisposition {
    match phase {
        PeerSessionPhase::Connected => ApplicationDataDisposition::Deliver,
        PeerSessionPhase::Negotiating if is_disconnect => ApplicationDataDisposition::Deliver,
        PeerSessionPhase::Negotiating if local_transport_ready && remote_ready => {
            ApplicationDataDisposition::Buffer
        }
        PeerSessionPhase::Negotiating => ApplicationDataDisposition::Reject,
        PeerSessionPhase::Requesting
        | PeerSessionPhase::Incoming
        | PeerSessionPhase::Disconnecting => ApplicationDataDisposition::Reject,
    }
}

fn validate_buffered_application_data(
    kind: ChannelKind,
    data: &[u8],
    max_message_bytes: usize,
) -> Result<(), PeerSessionError> {
    match kind {
        ChannelKind::Control => {
            let message: ControlMessage = serde_json::from_slice(data)
                .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
            message.validate()
        }
        ChannelKind::Messaging => {
            let message: MessagingMessage = serde_json::from_slice(data)
                .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
            message.validate(max_message_bytes)
        }
    }
}

fn require_connected_application_data(phase: PeerSessionPhase) -> Result<(), PeerSessionError> {
    if phase == PeerSessionPhase::Connected {
        Ok(())
    } else {
        Err(PeerSessionError::Protocol("application data arrived before session readiness".into()))
    }
}

fn command_error_is_terminal(error: &PeerSessionError, outcome_unknown_is_terminal: bool) -> bool {
    !matches!(
        error,
        PeerSessionError::InvalidState { .. }
            | PeerSessionError::EmptyMessage
            | PeerSessionError::MessageTooLarge { .. }
            | PeerSessionError::ShareMismatch(_)
    ) && (outcome_unknown_is_terminal || !matches!(error, PeerSessionError::OutcomeUnknown))
}

fn validate_remote_timestamp(
    timestamp: DateTime<Utc>,
    now: DateTime<Utc>,
    max_age: std::time::Duration,
    max_clock_skew: std::time::Duration,
) -> Result<(), PeerSessionError> {
    let max_age = chrono::Duration::from_std(max_age)
        .map_err(|_| PeerSessionError::Protocol("invalid remote timestamp age limit".into()))?;
    let max_clock_skew = chrono::Duration::from_std(max_clock_skew)
        .map_err(|_| PeerSessionError::Protocol("invalid remote clock skew limit".into()))?;
    if timestamp < now - max_age || timestamp > now + max_clock_skew {
        return Err(PeerSessionError::Protocol(
            "remote message timestamp is outside the accepted window".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_disconnect_has_a_bounded_recovery_window() {
        let since = Instant::now();
        let grace = std::time::Duration::from_secs(5);
        assert!(
            !disconnect_grace_expired(since, since + std::time::Duration::from_secs(4), grace,)
        );
        assert!(disconnect_grace_expired(since, since + grace, grace));
    }

    #[test]
    fn required_channel_close_is_terminal_before_and_after_readiness() {
        for phase in [PeerSessionPhase::Negotiating, PeerSessionPhase::Connected] {
            assert!(channel_close_is_terminal(phase));
        }
        assert!(!channel_close_is_terminal(PeerSessionPhase::Requesting));
        assert!(!channel_close_is_terminal(PeerSessionPhase::Incoming));
        assert!(!channel_close_is_terminal(PeerSessionPhase::Disconnecting));
    }

    #[test]
    fn local_candidates_are_only_forwarded_during_open_negotiation() {
        assert!(should_forward_local_candidate(PeerSessionPhase::Negotiating, true));
        assert!(!should_forward_local_candidate(PeerSessionPhase::Negotiating, false));
        assert!(!should_forward_local_candidate(PeerSessionPhase::Connected, false));
        assert!(!should_forward_local_candidate(PeerSessionPhase::Connected, true));
    }

    #[test]
    fn application_data_racing_the_final_ready_ack_is_buffered_in_order() {
        assert_eq!(
            application_data_disposition(PeerSessionPhase::Negotiating, false, true, true),
            ApplicationDataDisposition::Buffer
        );
        assert_eq!(
            application_data_disposition(PeerSessionPhase::Negotiating, true, false, false),
            ApplicationDataDisposition::Deliver
        );
        let mut pending = VecDeque::new();
        pending.push_back((ChannelKind::Messaging, bytes::Bytes::from_static(b"first")));
        pending.push_back((ChannelKind::Control, bytes::Bytes::from_static(b"second")));

        assert_eq!(
            application_data_disposition(PeerSessionPhase::Connected, false, false, false),
            ApplicationDataDisposition::Deliver
        );
        assert_eq!(pending.pop_front().unwrap().1, bytes::Bytes::from_static(b"first"));
        assert_eq!(pending.pop_front().unwrap().1, bytes::Bytes::from_static(b"second"));
        assert!(pending.is_empty());

        for phase in [
            PeerSessionPhase::Requesting,
            PeerSessionPhase::Incoming,
            PeerSessionPhase::Disconnecting,
        ] {
            assert_eq!(
                application_data_disposition(phase, false, true, true),
                ApplicationDataDisposition::Reject
            );
        }
        for (local_ready, remote_ready) in [(false, false), (true, false), (false, true)] {
            assert_eq!(
                application_data_disposition(
                    PeerSessionPhase::Negotiating,
                    false,
                    local_ready,
                    remote_ready,
                ),
                ApplicationDataDisposition::Reject
            );
        }
    }

    #[test]
    fn remote_timestamps_are_bounded() {
        let now = Utc::now();
        let age = std::time::Duration::from_secs(300);
        let skew = std::time::Duration::from_secs(30);
        assert!(validate_remote_timestamp(now, now, age, skew).is_ok());
        assert!(
            validate_remote_timestamp(now - chrono::Duration::hours(1), now, age, skew).is_err()
        );
        assert!(
            validate_remote_timestamp(now + chrono::Duration::hours(1), now, age, skew).is_err()
        );
    }

    #[test]
    fn ambiguous_message_send_keeps_session_open_for_receipt_reconciliation() {
        assert!(!command_error_is_terminal(&PeerSessionError::OutcomeUnknown, false,));
        assert!(command_error_is_terminal(&PeerSessionError::OutcomeUnknown, true,));
        assert!(command_error_is_terminal(
            &PeerSessionError::WebRtc("transport failed".into()),
            false,
        ));
    }

    #[tokio::test]
    async fn first_remote_video_source_keeps_samples_sent_before_subscription() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (_snapshot_tx, snapshot_rx) = watch::channel(PeerSessionSnapshot {
            session_id,
            peer_id,
            phase: PeerSessionPhase::Connected,
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        });
        let (encoded_video_sink, _encoded_video_rx) = encoded_video_channel(session_id, 1);
        let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(4);
        let (fatal_tx, _fatal_rx) = watch::channel(None);
        let handle = SessionActorHandle {
            session_id,
            generation: uuid::Uuid::new_v4(),
            command_tx,
            snapshot_rx,
            encoded_video_sink,
            remote_video_tx: remote_video_tx.clone(),
            initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
            fatal_tx,
        };
        let sample = super::super::EncodedVideoSample::new(
            bytes::Bytes::from_static(b"initial-idr"),
            std::time::Duration::from_millis(16),
        );

        remote_video_tx.send(sample.clone()).unwrap();
        let mut source = handle.remote_video_source();

        assert_eq!(source.recv().await.unwrap(), sample);
    }

    #[tokio::test]
    async fn fatal_control_bypasses_a_full_session_command_queue() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (reply, _reply_rx) = oneshot::channel();
        command_tx.send(SessionCommand::Accept(reply)).await.unwrap();
        let (_snapshot_tx, snapshot_rx) = watch::channel(PeerSessionSnapshot {
            session_id,
            peer_id,
            phase: PeerSessionPhase::Connected,
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        });
        let (encoded_video_sink, _video_rx) = encoded_video_channel(session_id, 1);
        let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
        let (fatal_tx, mut fatal_rx) = watch::channel(None);
        let handle = SessionActorHandle {
            session_id,
            generation: uuid::Uuid::new_v4(),
            command_tx,
            snapshot_rx,
            encoded_video_sink,
            remote_video_tx,
            initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
            fatal_tx,
        };

        handle.fail("mandatory sink failed");
        fatal_rx.changed().await.unwrap();
        assert_eq!(*fatal_rx.borrow(), Some(ActorControl::Fail("mandatory sink failed".into())));
    }

    #[tokio::test]
    async fn shutdown_control_bypasses_a_full_session_command_queue() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (reply, _reply_rx) = oneshot::channel();
        command_tx.send(SessionCommand::Accept(reply)).await.unwrap();
        let (_snapshot_tx, snapshot_rx) = watch::channel(PeerSessionSnapshot {
            session_id,
            peer_id,
            phase: PeerSessionPhase::Connected,
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        });
        let (encoded_video_sink, _video_rx) = encoded_video_channel(session_id, 1);
        let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
        let (fatal_tx, mut fatal_rx) = watch::channel(None);
        let handle = SessionActorHandle {
            session_id,
            generation: uuid::Uuid::new_v4(),
            command_tx,
            snapshot_rx,
            encoded_video_sink,
            remote_video_tx,
            initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
            fatal_tx,
        };
        let deadline = Instant::now() + std::time::Duration::from_secs(1);

        handle.shutdown(deadline);
        fatal_rx.changed().await.unwrap();
        assert_eq!(*fatal_rx.borrow(), Some(ActorControl::Shutdown { deadline }));
    }

    #[tokio::test]
    async fn trust_revocation_bypasses_a_full_session_command_queue() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (reply, _reply_rx) = oneshot::channel();
        command_tx.send(SessionCommand::Accept(reply)).await.unwrap();
        let (_snapshot_tx, snapshot_rx) = watch::channel(PeerSessionSnapshot {
            session_id,
            peer_id,
            phase: PeerSessionPhase::Connected,
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        });
        let (encoded_video_sink, _video_rx) = encoded_video_channel(session_id, 1);
        let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
        let (fatal_tx, mut fatal_rx) = watch::channel(None);
        let handle = SessionActorHandle {
            session_id,
            generation: uuid::Uuid::new_v4(),
            command_tx,
            snapshot_rx,
            encoded_video_sink,
            remote_video_tx,
            initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
            fatal_tx,
        };
        let deadline = Instant::now() + std::time::Duration::from_secs(1);

        handle.revoke_trust(deadline);
        fatal_rx.changed().await.unwrap();
        assert_eq!(*fatal_rx.borrow(), Some(ActorControl::TrustRevoked { deadline }));
    }

    #[tokio::test]
    async fn queued_unstarted_commands_receive_a_definitive_shutdown_error() {
        let (command_tx, mut command_rx) = mpsc::channel(1);
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx.send(SessionCommand::Accept(reply_tx)).await.unwrap();

        reject_queued_session_commands(&mut command_rx);

        assert_eq!(reply_rx.await.unwrap(), Err(PeerSessionError::ServiceStopped));
        assert!(command_rx.is_closed());
    }
}
