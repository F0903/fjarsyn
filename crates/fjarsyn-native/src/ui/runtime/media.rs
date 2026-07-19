use std::{collections::BTreeMap, sync::Arc, time::Duration};

use fjarsyn_core::{
    capture_providers::{CaptureProvider, PlatformCaptureItem, PlatformCaptureProvider},
    config::Config,
    media::{frame::Frame, pixel_format::PixelFormat},
    peer_session::{
        EncodedVideoSample, EncodedVideoSink, LocalShareState, PeerSessionServiceHandle,
        PeerSessionServiceSnapshot, RemoteShareState, RemoteVideoRead, RemoteVideoSource,
        SessionId, ShareEpoch, ShareId,
    },
    services::codec_service::{
        CodecDirectionState, DecoderSessionParts, DecoderWorkerConfig, EncoderSessionParts,
        EncoderWorkerConfig, Handle,
    },
};
use futures::StreamExt;
use tokio::{
    sync::{RwLock, mpsc, oneshot, watch},
    task::{AbortHandle, JoinHandle},
};

pub(super) const PIPELINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LocalMediaState {
    #[default]
    Inactive,
    Selecting,
    Starting,
    Active,
    Stopping,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RemoteMediaState {
    #[default]
    Inactive,
    Starting,
    Active,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCodecDirection {
    Encoder,
    Decoder,
}

/// Exact identity of one remote screen-share media generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShareMediaBinding {
    pub share_id: ShareId,
    pub epoch: ShareEpoch,
}

#[derive(Debug, Clone, Default)]
pub struct MediaSessionProjection {
    pub local: LocalMediaState,
    pub remote: RemoteMediaState,
    pub local_frame: Option<Arc<Frame>>,
    pub local_frame_binding: Option<ShareMediaBinding>,
    pub remote_frame: Option<Arc<Frame>>,
    pub remote_frame_binding: Option<ShareMediaBinding>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaProjection {
    pub sessions: Arc<BTreeMap<SessionId, MediaSessionProjection>>,
    encoder_restart_required: bool,
    decoder_restart_required: bool,
}

impl MediaProjection {
    pub fn session(&self, session_id: SessionId) -> MediaSessionProjection {
        self.sessions.get(&session_id).cloned().unwrap_or_default()
    }

    pub fn encoder_restart_required(&self) -> bool {
        self.encoder_restart_required
    }

    pub fn decoder_restart_required(&self) -> bool {
        self.decoder_restart_required
    }

    pub fn codec_restart_required(&self) -> bool {
        self.encoder_restart_required || self.decoder_restart_required
    }

    pub fn apply(&mut self, event: MediaEvent) {
        let sessions = Arc::make_mut(&mut self.sessions);
        match event {
            MediaEvent::LocalState { session_id, state } => {
                if self.encoder_restart_required {
                    return;
                }
                let projection = sessions.entry(session_id).or_default();
                projection.local = state;
                if matches!(
                    projection.local,
                    LocalMediaState::Inactive
                        | LocalMediaState::Stopping
                        | LocalMediaState::Failed(_)
                ) {
                    projection.local_frame = None;
                    projection.local_frame_binding = None;
                }
            }
            MediaEvent::RemoteState { session_id, state } => {
                if self.decoder_restart_required {
                    return;
                }
                let projection = sessions.entry(session_id).or_default();
                projection.remote = state;
                if matches!(
                    projection.remote,
                    RemoteMediaState::Inactive | RemoteMediaState::Failed(_)
                ) {
                    projection.remote_frame = None;
                    projection.remote_frame_binding = None;
                }
            }
            MediaEvent::LocalFrame { session_id, binding, frame } => {
                if self.encoder_restart_required {
                    return;
                }
                let projection = sessions.entry(session_id).or_default();
                projection.local_frame = Some(frame);
                projection.local_frame_binding = Some(binding);
            }
            MediaEvent::RemoteFrame { session_id, binding, frame } => {
                if self.decoder_restart_required {
                    return;
                }
                let projection = sessions.entry(session_id).or_default();
                projection.remote_frame = Some(frame);
                projection.remote_frame_binding = Some(binding);
            }
            MediaEvent::CodecRestartRequired { direction } => match direction {
                MediaCodecDirection::Encoder if !self.encoder_restart_required => {
                    self.encoder_restart_required = true;
                    for projection in sessions.values_mut() {
                        projection.local_frame = None;
                        projection.local_frame_binding = None;
                        if !matches!(projection.local, LocalMediaState::Inactive) {
                            projection.local = LocalMediaState::Failed(
                                "the video encoder is unavailable until Fjarsyn restarts".into(),
                            );
                        }
                    }
                }
                MediaCodecDirection::Decoder if !self.decoder_restart_required => {
                    self.decoder_restart_required = true;
                    for projection in sessions.values_mut() {
                        projection.remote_frame = None;
                        projection.remote_frame_binding = None;
                        if !matches!(projection.remote, RemoteMediaState::Inactive) {
                            projection.remote = RemoteMediaState::Failed(
                                "the video decoder is unavailable until Fjarsyn restarts".into(),
                            );
                        }
                    }
                }
                MediaCodecDirection::Encoder | MediaCodecDirection::Decoder => {}
            },
            MediaEvent::SessionClosed { session_id } => {
                sessions.remove(&session_id);
            }
        }
    }

    /// Reconciles decoded frames against the authenticated share-control state.
    /// A queued decoder event can arrive after `ShareStopped`; keeping the frame
    /// hidden and clearing it here prevents stale media from outliving the share.
    pub fn reconcile_shares(&mut self, snapshot: &PeerSessionServiceSnapshot) {
        let sessions = Arc::make_mut(&mut self.sessions);
        for (session_id, projection) in sessions {
            let session = snapshot.session(*session_id);
            let active_local_binding = session.and_then(|session| match session.local_share {
                LocalShareState::Active { share_id, epoch } => {
                    Some(ShareMediaBinding { share_id, epoch })
                }
                LocalShareState::Inactive => None,
            });
            if projection.local_frame_binding != active_local_binding {
                projection.local_frame = None;
                projection.local_frame_binding = None;
            }

            let active_remote_binding = session.and_then(|session| match session.remote_share {
                RemoteShareState::Active { share_id, epoch } => {
                    Some(ShareMediaBinding { share_id, epoch })
                }
                RemoteShareState::Inactive => None,
            });
            if projection.remote_frame_binding != active_remote_binding {
                projection.remote_frame = None;
                projection.remote_frame_binding = None;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MediaEvent {
    LocalState { session_id: SessionId, state: LocalMediaState },
    RemoteState { session_id: SessionId, state: RemoteMediaState },
    LocalFrame { session_id: SessionId, binding: ShareMediaBinding, frame: Arc<Frame> },
    RemoteFrame { session_id: SessionId, binding: ShareMediaBinding, frame: Arc<Frame> },
    CodecRestartRequired { direction: MediaCodecDirection },
    SessionClosed { session_id: SessionId },
}

struct OwnedPipeline {
    stop: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
    children: Vec<AbortHandle>,
}

impl OwnedPipeline {
    fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    async fn shutdown(mut self) {
        self.shutdown_with_timeout(PIPELINE_SHUTDOWN_TIMEOUT).await;
    }

    fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
    }

    async fn shutdown_with_timeout(&mut self, timeout: Duration) {
        self.shutdown_until(tokio::time::Instant::now() + timeout).await;
    }

    async fn shutdown_until(&mut self, deadline: tokio::time::Instant) {
        self.request_stop();
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if error.is_cancelled() => {}
                Ok(Err(error)) => tracing::warn!("media pipeline task failed: {error}"),
                Err(_) => {
                    tracing::warn!(
                        "media pipeline exceeded its shared shutdown deadline; aborting async workers"
                    );
                    self.abort_children();
                    task.abort();
                    let _ = task.await;
                }
            }
        }
    }

    fn abort_children(&self) {
        for child in &self.children {
            child.abort();
        }
    }
}

impl Drop for OwnedPipeline {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
        self.abort_children();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

struct ChildTaskGuard {
    aborts: Vec<AbortHandle>,
    armed: bool,
}

impl ChildTaskGuard {
    fn new(aborts: Vec<AbortHandle>) -> Self {
        Self { aborts, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ChildTaskGuard {
    fn drop(&mut self) {
        if self.armed {
            for abort in &self.aborts {
                abort.abort();
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LocalShareBinding {
    pub(crate) session_id: SessionId,
    pub(crate) share_id: ShareId,
    pub(crate) epoch: ShareEpoch,
}

impl LocalShareBinding {
    fn media(self) -> ShareMediaBinding {
        ShareMediaBinding { share_id: self.share_id, epoch: self.epoch }
    }
}

struct LocalPipeline {
    binding: LocalShareBinding,
    capture: Arc<RwLock<PlatformCaptureProvider>>,
    worker: OwnedPipeline,
    stop_requested: bool,
}

struct PendingLocalStart {
    session_id: SessionId,
    expires_at: tokio::time::Instant,
}

#[derive(Debug, Default)]
pub(super) struct LocalReconciliation {
    pub(super) teardown_native: Option<LocalShareBinding>,
    pub(super) stop_core: Vec<LocalShareBinding>,
    confirmed_stop: Option<LocalShareBinding>,
}

struct RemotePipeline {
    binding: ShareMediaBinding,
    worker: OwnedPipeline,
    source_return: oneshot::Receiver<RemoteVideoSource>,
}

pub struct SessionMediaService {
    event_tx: mpsc::Sender<super::RuntimeEvent>,
    sessions: PeerSessionServiceHandle,
    codecs: Handle,
    encoder_restart_required: bool,
    decoder_restart_required: bool,
    pending_local_start: Option<PendingLocalStart>,
    pending_local_stop: Option<LocalShareBinding>,
    local: Option<LocalPipeline>,
    standby_remote: BTreeMap<SessionId, RemoteVideoSource>,
    remote: BTreeMap<SessionId, RemotePipeline>,
}

impl std::fmt::Debug for SessionMediaService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionMediaService")
            .field(
                "local_session",
                &self.local.as_ref().map(|pipeline| pipeline.binding.session_id),
            )
            .field("standby_remote_sessions", &self.standby_remote.keys().collect::<Vec<_>>())
            .field("remote_sessions", &self.remote.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SessionMediaService {
    pub fn new(
        event_tx: mpsc::Sender<super::RuntimeEvent>,
        sessions: PeerSessionServiceHandle,
        codecs: Handle,
    ) -> Self {
        Self {
            event_tx,
            sessions,
            codecs,
            encoder_restart_required: false,
            decoder_restart_required: false,
            pending_local_start: None,
            pending_local_stop: None,
            local: None,
            standby_remote: BTreeMap::new(),
            remote: BTreeMap::new(),
        }
    }

    pub async fn mark_selecting(&self, session_id: SessionId) {
        if self.encoder_restart_required {
            self.emit(MediaEvent::LocalState {
                session_id,
                state: LocalMediaState::Failed(
                    "the video encoder is unavailable until Fjarsyn restarts".into(),
                ),
            })
            .await;
            return;
        }
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Selecting }).await;
    }

    pub async fn begin_local_start(&mut self, session_id: SessionId) -> Result<(), String> {
        if self.encoder_restart_required {
            let reason = "the video encoder is unavailable until Fjarsyn restarts".to_owned();
            self.emit(MediaEvent::LocalState {
                session_id,
                state: LocalMediaState::Failed(reason.clone()),
            })
            .await;
            return Err(reason);
        }
        self.pending_local_stop = None;
        self.pending_local_start = Some(PendingLocalStart {
            session_id,
            expires_at: tokio::time::Instant::now() + Duration::from_secs(30),
        });
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Starting }).await;
        Ok(())
    }

    pub async fn cancel_local(&mut self, session_id: SessionId) {
        if self.pending_local_start.as_ref().is_some_and(|pending| pending.session_id == session_id)
        {
            self.pending_local_start = None;
        }
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Inactive }).await;
    }

    pub async fn fail_local(&mut self, session_id: SessionId, reason: String) {
        if self.pending_local_start.as_ref().is_some_and(|pending| pending.session_id == session_id)
        {
            self.pending_local_start = None;
        }
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Failed(reason) })
            .await;
    }

    pub async fn start_local(
        &mut self,
        item: PlatformCaptureItem,
        sink: EncodedVideoSink,
        config: Config,
    ) -> Result<(), String> {
        if self.encoder_restart_required {
            return Err("the video encoder is unavailable until Fjarsyn restarts".into());
        }
        let session_id = sink.session_id();
        let share_id = sink.share_id();
        if self.pending_local_start.as_ref().is_some_and(|pending| pending.session_id == session_id)
        {
            self.pending_local_start = None;
        }
        if self.local.as_ref().is_some_and(|pipeline| pipeline.worker.is_finished()) {
            let stale = self.local.take().expect("finished local pipeline disappeared");
            stale.worker.shutdown().await;
            let _ = stale.capture.write().await.stop_capture();
        }
        if let Some(active) = &self.local {
            return Err(format!(
                "screen sharing is already active for session {}",
                active.binding.session_id
            ));
        }

        let binding = LocalShareBinding { session_id, share_id, epoch: sink.epoch() };

        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Starting }).await;

        let provider = fjarsyn_core::capture_providers::windows::WgcCaptureProviderBuilder::new(
            PixelFormat::DEFAULT_CAPTURE,
            config.capture.record_cursor,
            config.capture.recording_border_indicator,
            fjarsyn_core::config::requires_capture_readback(&config),
        )
        .with_default_device()
        .and_then(|builder| builder.with_default_capture_item())
        .and_then(|builder| builder.build())
        .map_err(|error| error.to_string())?;
        let capture = Arc::new(RwLock::new(provider));

        {
            let mut provider = capture.write().await;
            provider.set_capture_item(item).map_err(|error| error.to_string())?;
            provider.start_capture().map_err(|error| error.to_string())?;
        }

        let mut stream = match capture.write().await.create_stream(config.video.target_framerate) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = capture.write().await.stop_capture();
                return Err(error.to_string());
            }
        };
        let encoder = match self
            .codecs
            .open_encoder(EncoderWorkerConfig {
                bitrate: config.video.target_bitrate,
                target_framerate_hz: config.video.target_framerate.to_hz(),
                target_resolution: config.video.target_resolution,
                input_format: PixelFormat::DEFAULT_CAPTURE,
                device: capture.read().await.codec_device(),
                transcoding_type: config.video.transcoding_type,
            })
            .await
        {
            Ok(encoder) => encoder,
            Err(error) => {
                let _ = capture.write().await.stop_capture();
                return Err(error.to_string());
            }
        };
        let EncoderSessionParts {
            input: encoder_input,
            output: mut encoder_output,
            worker: encoder_worker,
        } = encoder.into_parts();

        let project_local_preview = config.capture.enable_ui_preview;
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let mut capture_cancel = cancel_rx.clone();
        let capture_cancel_tx = cancel_tx.clone();
        let capture_events = self.event_tx.clone();
        let capture_task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = capture_cancel.changed() => return Ok::<(), String>(()),
                    frame = stream.next() => match frame {
                        Some(frame) => frame,
                        None => {
                            let _ = capture_cancel_tx.send(true);
                            return Err("the capture stream ended unexpectedly".into());
                        }
                    },
                };
                let frame = Arc::new(frame);
                if project_local_preview {
                    let _ = capture_events.try_send(super::RuntimeEvent::Media(
                        MediaEvent::LocalFrame {
                            session_id,
                            binding: binding.media(),
                            frame: frame.clone(),
                        },
                    ));
                }
                match encoder_input.try_send(frame) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                }
            }
        });

        // The service owns the dedicated OS thread and watchdog. This async task
        // observes only its terminal result; dropping it requests a non-blocking
        // stop and never joins an in-flight native call on a Tokio worker.
        let encoder_cancel_tx = cancel_tx.clone();
        let encoder_task = tokio::spawn(async move {
            let result = encoder_worker
                .wait()
                .await
                .map_err(|error| format!("encoder worker failed: {error}"));
            let _ = encoder_cancel_tx.send(true);
            result
        });

        let mut network_cancel = cancel_rx;
        let network_cancel_tx = cancel_tx.clone();
        let encoder_health = self.codecs.clone();
        let network_task = tokio::spawn(async move {
            loop {
                let encoded = tokio::select! {
                    biased;
                    _ = network_cancel.changed() => return Ok::<(), String>(()),
                    encoded = encoder_output.recv() => match encoded {
                        Some(Ok(encoded)) => encoded,
                        Some(Err(error)) => {
                            let _ = network_cancel_tx.send(true);
                            return Err(format!("failed to encode screen frame: {error}"));
                        }
                        None => return Ok(()),
                    },
                };
                for nal in encoded.nal_units {
                    if matches!(
                        encoder_health.snapshot().encode,
                        CodecDirectionState::RestartRequired(_)
                    ) {
                        return Ok(());
                    }
                    let sample = EncodedVideoSample::new(nal, encoded.duration);
                    tokio::select! {
                        biased;
                        _ = network_cancel.changed() => return Ok(()),
                        result = sink.send(sample) => {
                            if let Err(error) = result {
                                let _ = network_cancel_tx.send(true);
                                return Err(format!("video transport closed: {error}"));
                            }
                        }
                    }
                }
            }
        });

        let pipeline_events = self.event_tx.clone();
        let failure_capture = capture.clone();
        let child_aborts = vec![
            capture_task.abort_handle(),
            encoder_task.abort_handle(),
            network_task.abort_handle(),
        ];
        let supervisor_aborts = child_aborts.clone();
        let task = tokio::spawn(async move {
            let mut child_guard = ChildTaskGuard::new(supervisor_aborts);
            let (capture, encoder, network) =
                tokio::join!(capture_task, encoder_task, network_task);
            child_guard.disarm();
            let failure = [
                pipeline_task_failure("capture", capture),
                pipeline_task_failure("encoder", encoder),
                pipeline_task_failure("video transport", network),
            ]
            .into_iter()
            .flatten()
            .next();
            let state = match failure {
                Some(reason) => {
                    // Stop capture immediately. The application reconciliation
                    // worker supervises the exact ShareId until core confirms
                    // it inactive or the session disappears.
                    if let Err(error) = failure_capture.write().await.stop_capture() {
                        tracing::warn!("failed to stop capture after media failure: {error}");
                    }
                    LocalMediaState::Failed(reason)
                }
                None => LocalMediaState::Inactive,
            };
            let _ = pipeline_events
                .send(super::RuntimeEvent::Media(MediaEvent::LocalState {
                    session_id: binding.session_id,
                    state,
                }))
                .await;
        });

        self.local = Some(LocalPipeline {
            binding,
            capture,
            worker: OwnedPipeline {
                stop: Some(cancel_tx),
                task: Some(task),
                children: child_aborts,
            },
            stop_requested: false,
        });
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Active }).await;
        Ok(())
    }

    pub(crate) async fn request_local_stop(
        &mut self,
        session_id: SessionId,
    ) -> Option<LocalShareBinding> {
        let binding = if let Some(pipeline) =
            self.local.as_mut().filter(|pipeline| pipeline.binding.session_id == session_id)
        {
            self.pending_local_stop = None;
            pipeline.stop_requested = true;
            // Privacy does not wait for the authenticated ShareStopped exchange.
            // Stop capture and the outbound transport immediately, but retain the
            // exact binding until core confirms that this ShareId is inactive.
            pipeline.worker.request_stop();
            if let Err(error) = pipeline.capture.write().await.stop_capture() {
                tracing::warn!(%error, "failed to stop capture while requesting share stop");
            }
            Some(pipeline.binding)
        } else {
            self.sessions.snapshot().session(session_id).and_then(|session| {
                match session.local_share {
                    LocalShareState::Active { share_id, epoch } => {
                        Some(LocalShareBinding { session_id, share_id, epoch })
                    }
                    LocalShareState::Inactive => None,
                }
            })
        };
        if self.local.is_none() {
            self.pending_local_stop = binding;
        }
        let state =
            if binding.is_some() { LocalMediaState::Stopping } else { LocalMediaState::Inactive };
        self.emit(MediaEvent::LocalState { session_id, state }).await;
        binding
    }

    pub(super) fn local_reconciliation(&mut self) -> LocalReconciliation {
        let now = tokio::time::Instant::now();
        if self.pending_local_start.as_ref().is_some_and(|pending| pending.expires_at <= now)
            && let Some(pending) = self.pending_local_start.take()
        {
            let _ = self.event_tx.try_send(super::RuntimeEvent::Media(MediaEvent::LocalState {
                session_id: pending.session_id,
                state: LocalMediaState::Failed(
                    "screen-share startup did not complete in time".into(),
                ),
            }));
        }

        let active_core = self
            .sessions
            .snapshot()
            .sessions
            .iter()
            .filter_map(|session| match session.local_share {
                LocalShareState::Active { share_id, epoch } => {
                    Some(LocalShareBinding { session_id: session.session_id, share_id, epoch })
                }
                LocalShareState::Inactive => None,
            })
            .collect::<Vec<_>>();
        let native = self.local.as_ref().map(|pipeline| NativeLocalShare {
            binding: pipeline.binding,
            stop_requested: pipeline.stop_requested || pipeline.worker.is_finished(),
        });
        let pending_session = self.pending_local_start.as_ref().map(|pending| pending.session_id);
        let plan = plan_local_reconciliation(
            &active_core,
            native,
            pending_session,
            self.pending_local_stop,
        );
        if plan.confirmed_stop.is_some_and(|binding| {
            self.pending_local_stop.is_some_and(|pending| pending == binding)
        }) && let Some(binding) = self.pending_local_stop.take()
        {
            let _ = self.event_tx.try_send(super::RuntimeEvent::Media(MediaEvent::LocalState {
                session_id: binding.session_id,
                state: LocalMediaState::Inactive,
            }));
        }
        plan
    }

    pub(super) async fn teardown_local(&mut self, binding: LocalShareBinding) {
        let Some(pipeline) = self.local.take() else {
            return;
        };
        if pipeline.binding != binding {
            self.local = Some(pipeline);
            return;
        }
        pipeline.worker.shutdown().await;
        if let Err(error) = pipeline.capture.write().await.stop_capture() {
            tracing::warn!("failed to stop capture: {error}");
        }
        self.emit(MediaEvent::LocalState {
            session_id: binding.session_id,
            state: LocalMediaState::Inactive,
        })
        .await;
    }

    async fn teardown_local_session(&mut self, session_id: SessionId) {
        if let Some(binding) = self
            .local
            .as_ref()
            .filter(|pipeline| pipeline.binding.session_id == session_id)
            .map(|pipeline| pipeline.binding)
        {
            self.teardown_local(binding).await;
        } else {
            self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Inactive })
                .await;
        }
    }

    pub async fn start_remote(
        &mut self,
        session_id: SessionId,
        binding: ShareMediaBinding,
        config: Config,
    ) -> Result<(), String> {
        if self.decoder_restart_required {
            return Err("the video decoder is unavailable until Fjarsyn restarts".into());
        }
        if self
            .remote
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.binding == binding && !pipeline.worker.is_finished())
        {
            return Ok(());
        }
        if let Some(stale) = self.remote.remove(&session_id)
            && let Some(source) = shutdown_remote_pipeline(stale).await
        {
            self.standby_remote.insert(session_id, source);
        }
        let Some(mut source) = self.standby_remote.remove(&session_id) else {
            let reason = "remote video standby source is unavailable".to_owned();
            self.emit(MediaEvent::RemoteState {
                session_id,
                state: RemoteMediaState::Failed(reason.clone()),
            })
            .await;
            return Err(reason);
        };
        let decoder = match self
            .codecs
            .open_decoder(DecoderWorkerConfig {
                transcoding_type: config.video.transcoding_type,
                output_format: PixelFormat::DEFAULT_CAPTURE,
            })
            .await
        {
            Ok(decoder) => decoder,
            Err(error) => {
                self.standby_remote.insert(session_id, source);
                let reason = error.to_string();
                self.emit(MediaEvent::RemoteState {
                    session_id,
                    state: RemoteMediaState::Failed(reason.clone()),
                })
                .await;
                return Err(reason);
            }
        };
        let DecoderSessionParts {
            input: decoder_input,
            output: mut decoder_output,
            worker: decoder_worker,
        } = decoder.into_parts();
        self.emit(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Starting }).await;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (source_return_tx, source_return) = oneshot::channel();

        let mut source_cancel = cancel_rx.clone();
        let source_cancel_tx = cancel_tx.clone();
        let source_task = tokio::spawn(async move {
            let mut found_sequence_parameter_set = false;
            let result = loop {
                let sample = tokio::select! {
                    biased;
                    _ = source_cancel.changed() => break Ok::<(), String>(()),
                    sample = source.recv_for(binding.epoch) => match sample {
                        Ok(RemoteVideoRead::Sample(sample)) => sample,
                        Ok(RemoteVideoRead::EpochAdvanced { next_epoch }) => {
                            tracing::debug!(
                                %session_id,
                                share_epoch = binding.epoch.value(),
                                next_share_epoch = next_epoch.value(),
                                "remote media advanced before its control event was projected; parking the old decoder"
                            );
                            // Keep this pipeline alive until reconciliation sees
                            // the new control epoch. Completing here would cause
                            // the old binding to be recreated every tick while
                            // the future sample remains pending in the source.
                            park_until_pipeline_replacement(&mut source_cancel).await;
                            break Ok(());
                        }
                        Err(fjarsyn_core::peer_session::PeerSessionError::RemoteVideoLagged { skipped }) => {
                            tracing::debug!(%session_id, skipped, "remote video source lagged; continuing at the retained media boundary");
                            continue;
                        }
                        Err(error) => {
                            let _ = source_cancel_tx.send(true);
                            break Err(error.to_string());
                        }
                    },
                };
                // Media identity was fenced by the RTP epoch before this
                // decoder queue. SPS gating is solely codec bootstrap: wait
                // for the new encoder's parameters before decoding.
                if !found_sequence_parameter_set {
                    found_sequence_parameter_set = contains_h264_nal_type(&sample.data, 7);
                    if !found_sequence_parameter_set {
                        continue;
                    }
                }
                match decoder_input.try_send(sample.data) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => break Ok(()),
                }
            };
            if result.is_ok() {
                let _ = source_return_tx.send(source);
            }
            result
        });

        let decoder_cancel_tx = cancel_tx.clone();
        let decoder_task = tokio::spawn(async move {
            let result = decoder_worker
                .wait()
                .await
                .map_err(|error| format!("decoder worker failed: {error}"));
            let _ = decoder_cancel_tx.send(true);
            result
        });

        let mut projection_cancel = cancel_rx;
        let projection_events = self.event_tx.clone();
        let decoder_health = self.codecs.clone();
        let projection_task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = projection_cancel.changed() => return Ok::<(), String>(()),
                    frame = decoder_output.recv() => match frame {
                        Some(Ok(frame)) => frame,
                        Some(Err(error)) => return Err(format!("failed to decode remote video: {error}")),
                        None => return Ok(()),
                    },
                };
                if matches!(
                    decoder_health.snapshot().decode,
                    CodecDirectionState::RestartRequired(_)
                ) {
                    return Ok(());
                }
                let _ = projection_events.try_send(super::RuntimeEvent::Media(
                    MediaEvent::RemoteFrame { session_id, binding, frame },
                ));
            }
        });

        let pipeline_events = self.event_tx.clone();
        let child_aborts = vec![
            source_task.abort_handle(),
            decoder_task.abort_handle(),
            projection_task.abort_handle(),
        ];
        let supervisor_aborts = child_aborts.clone();
        let task = tokio::spawn(async move {
            let mut child_guard = ChildTaskGuard::new(supervisor_aborts);
            let (source, decoder, projection) =
                tokio::join!(source_task, decoder_task, projection_task);
            child_guard.disarm();
            let failure = [
                pipeline_task_failure("remote video source", source),
                pipeline_task_failure("decoder", decoder),
                pipeline_task_failure("frame projection", projection),
            ]
            .into_iter()
            .flatten()
            .next();
            let state = failure.map(RemoteMediaState::Failed).unwrap_or(RemoteMediaState::Inactive);
            let _ = pipeline_events
                .send(super::RuntimeEvent::Media(MediaEvent::RemoteState { session_id, state }))
                .await;
        });

        self.remote.insert(
            session_id,
            RemotePipeline {
                binding,
                worker: OwnedPipeline {
                    stop: Some(cancel_tx),
                    task: Some(task),
                    children: child_aborts,
                },
                source_return,
            },
        );
        self.emit(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Active }).await;
        Ok(())
    }

    pub fn remote_receiver_ready(&self, session_id: SessionId) -> bool {
        self.decoder_restart_required
            || self.standby_remote.contains_key(&session_id)
            || self.remote.contains_key(&session_id)
    }

    pub fn encoder_restart_required(&self) -> bool {
        self.encoder_restart_required
    }

    pub fn decoder_restart_required(&self) -> bool {
        self.decoder_restart_required
    }

    pub fn install_standby_remote(&mut self, session_id: SessionId, source: RemoteVideoSource) {
        if !self.decoder_restart_required && !self.remote_receiver_ready(session_id) {
            self.standby_remote.insert(session_id, source);
        }
    }

    pub fn remote_is_running(&self, session_id: SessionId, binding: ShareMediaBinding) -> bool {
        self.remote
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.binding == binding && !pipeline.worker.is_finished())
    }

    pub async fn stop_session(&mut self, session_id: SessionId) {
        if self.pending_local_stop.is_some_and(|binding| binding.session_id == session_id) {
            self.pending_local_stop = None;
        }
        self.teardown_local_session(session_id).await;
        self.stop_remote(session_id).await;
        self.standby_remote.remove(&session_id);
        self.emit(MediaEvent::SessionClosed { session_id }).await;
    }

    pub async fn stop_remote(&mut self, session_id: SessionId) {
        if let Some(pipeline) = self.remote.remove(&session_id)
            && let Some(source) = shutdown_remote_pipeline(pipeline).await
        {
            self.standby_remote.insert(session_id, source);
        }
        self.emit(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Inactive }).await;
    }

    pub fn require_codec_restart(&mut self, direction: MediaCodecDirection) -> bool {
        match direction {
            MediaCodecDirection::Encoder if !self.encoder_restart_required => {
                self.encoder_restart_required = true;
                self.pending_local_start = None;
                if let Some(local) = self.local.as_mut() {
                    local.stop_requested = true;
                    local.worker.request_stop();
                    stop_capture_outside_runtime(local.capture.clone());
                }
                true
            }
            MediaCodecDirection::Decoder if !self.decoder_restart_required => {
                self.decoder_restart_required = true;
                self.standby_remote.clear();
                let mut remote = std::mem::take(&mut self.remote);
                for pipeline in remote.values_mut() {
                    pipeline.worker.request_stop();
                }
                drop(remote);
                true
            }
            MediaCodecDirection::Encoder | MediaCodecDirection::Decoder => false,
        }
    }

    pub async fn shutdown_until(&mut self, deadline: tokio::time::Instant) {
        self.pending_local_start = None;
        self.pending_local_stop = None;
        let mut local = self.local.take();
        let mut remote = std::mem::take(&mut self.remote);

        // Signal every pipeline before awaiting any of them. All workers share
        // one absolute deadline, so shutdown time is independent of pipeline count.
        if let Some(local) = local.as_mut() {
            local.worker.request_stop();
        }
        for pipeline in remote.values_mut() {
            pipeline.worker.request_stop();
        }

        let local_shutdown = async {
            if let Some(local) = local {
                let LocalPipeline { mut worker, capture, .. } = local;
                worker.shutdown_until(deadline).await;
                stop_capture_outside_runtime(capture);
            }
        };
        let remote_shutdown = futures::future::join_all(
            remote.values_mut().map(|pipeline| pipeline.worker.shutdown_until(deadline)),
        );
        tokio::join!(local_shutdown, remote_shutdown);
        self.standby_remote.clear();
    }

    async fn emit(&self, event: MediaEvent) {
        let _ = self.event_tx.send(super::RuntimeEvent::Media(event)).await;
    }

    pub(super) fn cancel_now(&mut self) {
        self.pending_local_start = None;
        self.pending_local_stop = None;
        if let Some(local) = self.local.take() {
            stop_capture_outside_runtime(local.capture);
            drop(local.worker);
        }
        self.standby_remote.clear();
        self.remote.clear();
    }
}

/// WGC teardown invokes synchronous COM close calls. Keep those calls away
/// from Tokio so application shutdown remains bounded even if a driver stalls.
fn stop_capture_outside_runtime(capture: Arc<RwLock<PlatformCaptureProvider>>) {
    // Retain a second reference until thread creation succeeds. If the OS
    // refuses a new cleanup thread, intentionally leak this shutdown-only
    // reference rather than synchronously dropping a potentially stuck WGC
    // provider on the async runtime.
    let fallback = capture.clone();
    let spawn =
        std::thread::Builder::new().name("fjarsyn-capture-cleanup".into()).spawn(move || {
            use windows::Win32::System::Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
            };

            if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
                tracing::warn!(%error, "failed to initialize capture cleanup COM apartment; deferring cleanup to process exit");
                std::mem::forget(capture);
                return;
            }
            {
                let mut provider = capture.blocking_write();
                if let Err(error) = provider.stop_capture() {
                    tracing::warn!(%error, "failed to stop capture during detached cleanup");
                }
            }
            drop(capture);
            unsafe { CoUninitialize() };
        });
    if let Err(error) = spawn {
        tracing::warn!(%error, "failed to spawn capture cleanup thread; deferring cleanup to process exit");
        std::mem::forget(fallback);
    }
}

async fn shutdown_remote_pipeline(pipeline: RemotePipeline) -> Option<RemoteVideoSource> {
    let RemotePipeline { mut worker, source_return, .. } = pipeline;
    worker.request_stop();
    let (_, source) = tokio::join!(
        worker.shutdown(),
        tokio::time::timeout(PIPELINE_SHUTDOWN_TIMEOUT, source_return)
    );
    match source {
        Ok(Ok(source)) => Some(source),
        Ok(Err(_)) => None,
        Err(_) => {
            tracing::warn!("remote video source was not returned before the shutdown deadline");
            None
        }
    }
}

async fn park_until_pipeline_replacement(cancel: &mut watch::Receiver<bool>) {
    let _ = cancel.changed().await;
}

fn contains_h264_nal_type(data: &[u8], target: u8) -> bool {
    let mut offset = 0;
    let mut found_annex_b = false;
    while offset + 3 <= data.len() {
        let start_code_len = if data[offset..].starts_with(&[0, 0, 0, 1]) {
            Some(4)
        } else if data[offset..].starts_with(&[0, 0, 1]) {
            Some(3)
        } else {
            None
        };
        if let Some(start_code_len) = start_code_len {
            found_annex_b = true;
            let nal_offset = offset + start_code_len;
            if data.get(nal_offset).is_some_and(|header| header & 0x1f == target) {
                return true;
            }
            offset = nal_offset.saturating_add(1);
        } else {
            offset += 1;
        }
    }
    if found_annex_b {
        return false;
    }

    // Accept AVCC length-prefixed access units as well as a single raw NAL.
    let mut offset = 0;
    let mut parsed_avcc = false;
    while offset + 4 <= data.len() {
        let size =
            u32::from_be_bytes(data[offset..offset + 4].try_into().expect("four bytes")) as usize;
        let nal_offset = offset + 4;
        let Some(end) = nal_offset.checked_add(size).filter(|end| *end <= data.len()) else {
            break;
        };
        if size == 0 {
            break;
        }
        parsed_avcc = true;
        if data[nal_offset] & 0x1f == target {
            return true;
        }
        offset = end;
    }
    if parsed_avcc && offset == data.len() {
        return false;
    }

    data.first().is_some_and(|header| header & 0x1f == target)
}

#[derive(Debug, Clone, Copy)]
struct NativeLocalShare {
    binding: LocalShareBinding,
    stop_requested: bool,
}

fn plan_local_reconciliation(
    active_core: &[LocalShareBinding],
    native: Option<NativeLocalShare>,
    pending_start_session: Option<SessionId>,
    pending_stop: Option<LocalShareBinding>,
) -> LocalReconciliation {
    let mut plan = LocalReconciliation::default();
    if let Some(native) = native {
        let exact_active = active_core.contains(&native.binding);
        if !exact_active {
            plan.teardown_native = Some(native.binding);
        } else if native.stop_requested {
            plan.stop_core.push(native.binding);
        }
        plan.stop_core
            .extend(active_core.iter().copied().filter(|binding| *binding != native.binding));
    } else {
        plan.stop_core.extend(
            active_core
                .iter()
                .copied()
                .filter(|binding| pending_start_session != Some(binding.session_id)),
        );
    }
    if let Some(pending_stop) = pending_stop {
        if active_core.contains(&pending_stop) {
            plan.stop_core.push(pending_stop);
        } else {
            plan.confirmed_stop = Some(pending_stop);
        }
    }
    plan.stop_core.sort_unstable();
    plan.stop_core.dedup();
    plan
}

impl Drop for SessionMediaService {
    fn drop(&mut self) {
        self.cancel_now();
    }
}

fn pipeline_task_failure(
    context: &str,
    result: Result<Result<(), String>, tokio::task::JoinError>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(error) if error.is_cancelled() => None,
        Err(error) => Some(format!("{context} worker failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use fjarsyn_core::{
        media::{frame::FrameData, pixel_format::PixelFormat},
        peer_session::{PeerId, PeerSessionPhase, PeerSessionServiceSnapshot, PeerSessionSnapshot},
        utils::vector2::Vector2,
    };

    use super::*;

    fn test_frame() -> Arc<Frame> {
        Arc::new(Frame {
            data: FrameData::Software(bytes::Bytes::new()),
            format: PixelFormat::BGRA8,
            size: Vector2::new(0, 0),
            duration: None,
        })
    }

    #[test]
    fn codec_quarantine_is_directional_and_rejects_late_frames() {
        let session_id = SessionId::new();
        let binding = ShareMediaBinding { share_id: ShareId::new(), epoch: ShareEpoch::FIRST };
        let mut projection = MediaProjection::default();
        projection.apply(MediaEvent::LocalState { session_id, state: LocalMediaState::Active });
        projection.apply(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Active });
        projection.apply(MediaEvent::LocalFrame { session_id, binding, frame: test_frame() });
        projection.apply(MediaEvent::RemoteFrame { session_id, binding, frame: test_frame() });

        projection
            .apply(MediaEvent::CodecRestartRequired { direction: MediaCodecDirection::Encoder });

        assert!(projection.encoder_restart_required());
        assert!(!projection.decoder_restart_required());
        assert!(projection.session(session_id).local_frame.is_none());
        assert!(projection.session(session_id).remote_frame.is_some());
        projection.apply(MediaEvent::LocalState { session_id, state: LocalMediaState::Inactive });
        assert!(matches!(projection.session(session_id).local, LocalMediaState::Failed(_)));
        projection.apply(MediaEvent::LocalFrame { session_id, binding, frame: test_frame() });
        assert!(projection.session(session_id).local_frame.is_none());

        projection
            .apply(MediaEvent::CodecRestartRequired { direction: MediaCodecDirection::Decoder });
        projection
            .apply(MediaEvent::CodecRestartRequired { direction: MediaCodecDirection::Decoder });

        assert!(projection.codec_restart_required());
        assert!(projection.session(session_id).remote_frame.is_none());
        projection.apply(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Inactive });
        assert!(matches!(projection.session(session_id).remote, RemoteMediaState::Failed(_)));
        projection.apply(MediaEvent::RemoteFrame { session_id, binding, frame: test_frame() });
        assert!(projection.session(session_id).remote_frame.is_none());

        projection.apply(MediaEvent::SessionClosed { session_id });
        assert!(projection.codec_restart_required());
        assert!(projection.sessions.is_empty());
    }

    #[test]
    fn ordinary_codec_failure_does_not_request_an_application_restart() {
        let session_id = SessionId::new();
        let mut projection = MediaProjection::default();

        projection.apply(MediaEvent::LocalState {
            session_id,
            state: LocalMediaState::Failed("ordinary encoder error".into()),
        });
        projection.apply(MediaEvent::RemoteState {
            session_id,
            state: RemoteMediaState::Failed("ordinary decoder error".into()),
        });

        assert!(!projection.codec_restart_required());
    }

    #[test]
    fn local_pipeline_cleanup_keeps_the_original_share_identity() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding = LocalShareBinding { session_id, share_id, epoch: ShareEpoch::FIRST };

        assert_eq!(binding.session_id, session_id);
        assert_eq!(binding.share_id, share_id);
    }

    #[test]
    fn local_reconciliation_supervises_stop_until_exact_share_is_inactive() {
        let binding = LocalShareBinding {
            session_id: SessionId::new(),
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };

        let plan = plan_local_reconciliation(
            &[binding],
            Some(NativeLocalShare { binding, stop_requested: true }),
            None,
            None,
        );

        assert_eq!(plan.stop_core, vec![binding]);
        assert_eq!(plan.teardown_native, None);
    }

    #[test]
    fn local_reconciliation_tears_down_native_only_after_core_is_inactive() {
        let binding = LocalShareBinding {
            session_id: SessionId::new(),
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };

        let plan = plan_local_reconciliation(
            &[],
            Some(NativeLocalShare { binding, stop_requested: true }),
            None,
            None,
        );

        assert_eq!(plan.teardown_native, Some(binding));
        assert!(plan.stop_core.is_empty());
    }

    #[test]
    fn pending_start_protects_ambiguous_share_but_orphans_are_stopped() {
        let pending_session = SessionId::new();
        let pending = LocalShareBinding {
            session_id: pending_session,
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };
        let orphan = LocalShareBinding {
            session_id: SessionId::new(),
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };

        let plan = plan_local_reconciliation(&[pending, orphan], None, Some(pending_session), None);

        assert_eq!(plan.stop_core, vec![orphan]);
    }

    #[test]
    fn changed_share_id_never_reuses_an_obsolete_native_pipeline() {
        let session_id = SessionId::new();
        let native_binding =
            LocalShareBinding { session_id, share_id: ShareId::new(), epoch: ShareEpoch::FIRST };
        let core_binding =
            LocalShareBinding { session_id, share_id: ShareId::new(), epoch: ShareEpoch::FIRST };

        let plan = plan_local_reconciliation(
            &[core_binding],
            Some(NativeLocalShare { binding: native_binding, stop_requested: false }),
            None,
            None,
        );

        assert_eq!(plan.teardown_native, Some(native_binding));
        assert_eq!(plan.stop_core, vec![core_binding]);
    }

    #[test]
    fn control_only_stop_intent_remains_until_the_exact_share_is_inactive() {
        let binding = LocalShareBinding {
            session_id: SessionId::new(),
            share_id: ShareId::new(),
            epoch: ShareEpoch::FIRST,
        };

        let pending = plan_local_reconciliation(&[binding], None, None, Some(binding));
        assert_eq!(pending.stop_core, vec![binding]);
        assert_eq!(pending.confirmed_stop, None);

        let confirmed = plan_local_reconciliation(&[], None, None, Some(binding));
        assert!(confirmed.stop_core.is_empty());
        assert_eq!(confirmed.confirmed_stop, Some(binding));
    }

    #[test]
    fn local_stop_signal_is_immediate_and_idempotent() {
        let (stop, mut stopped) = watch::channel(false);
        let mut pipeline = OwnedPipeline { stop: Some(stop), task: None, children: Vec::new() };

        pipeline.request_stop();
        assert!(*stopped.borrow_and_update());

        pipeline.request_stop();
        assert!(*stopped.borrow_and_update());
    }

    #[test]
    fn decoder_bootstrap_recognizes_h264_sequence_parameter_sets() {
        assert!(contains_h264_nal_type(&[0, 0, 0, 1, 0x67, 1, 2], 7));
        assert!(contains_h264_nal_type(&[0, 0, 0, 3, 0x67, 1, 2], 7));
        assert!(contains_h264_nal_type(&[0x67, 1, 2], 7));
        assert!(!contains_h264_nal_type(&[0, 0, 1, 0x65, 1, 2], 7));
    }

    #[test]
    fn remote_frames_require_the_exact_authenticated_share_epoch() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding_a = ShareMediaBinding { share_id, epoch: ShareEpoch::FIRST };
        let binding_b = ShareMediaBinding {
            share_id,
            epoch: ShareEpoch::try_from(ShareEpoch::FIRST.value() + 1).unwrap(),
        };
        let snapshot = |remote_share| PeerSessionServiceSnapshot {
            sessions: Arc::new(vec![PeerSessionSnapshot {
                session_id,
                peer_id: PeerId::new("peer-a").unwrap(),
                phase: PeerSessionPhase::Connected,
                local_share: LocalShareState::Inactive,
                remote_share,
            }]),
        };
        let active_a = snapshot(RemoteShareState::Active {
            share_id: binding_a.share_id,
            epoch: binding_a.epoch,
        });
        let active_b = snapshot(RemoteShareState::Active {
            share_id: binding_b.share_id,
            epoch: binding_b.epoch,
        });
        let stopped = snapshot(RemoteShareState::Inactive);
        let mut projection = MediaProjection::default();

        projection.apply(MediaEvent::RemoteFrame {
            session_id,
            binding: binding_a,
            frame: test_frame(),
        });
        projection.reconcile_shares(&active_a);
        assert!(projection.session(session_id).remote_frame.is_some());

        projection.reconcile_shares(&active_b);
        assert!(projection.session(session_id).remote_frame.is_none());

        projection.apply(MediaEvent::RemoteFrame {
            session_id,
            binding: binding_a,
            frame: test_frame(),
        });
        projection.reconcile_shares(&active_b);
        assert!(projection.session(session_id).remote_frame.is_none());

        projection.apply(MediaEvent::RemoteFrame {
            session_id,
            binding: binding_b,
            frame: test_frame(),
        });
        projection.reconcile_shares(&active_b);
        assert!(projection.session(session_id).remote_frame.is_some());

        projection.reconcile_shares(&stopped);
        projection.apply(MediaEvent::RemoteFrame {
            session_id,
            binding: binding_b,
            frame: test_frame(),
        });
        projection.reconcile_shares(&stopped);

        let session = projection.session(session_id);
        assert!(session.remote_frame.is_none());
        assert_eq!(session.remote_frame_binding, None);
    }

    #[test]
    fn local_preview_frames_require_the_exact_share_epoch() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding_a = ShareMediaBinding { share_id, epoch: ShareEpoch::FIRST };
        let binding_b = ShareMediaBinding {
            share_id,
            epoch: ShareEpoch::try_from(ShareEpoch::FIRST.value() + 1).unwrap(),
        };
        let snapshot = |binding: Option<ShareMediaBinding>| PeerSessionServiceSnapshot {
            sessions: Arc::new(vec![PeerSessionSnapshot {
                session_id,
                peer_id: PeerId::new("peer-a").unwrap(),
                phase: PeerSessionPhase::Connected,
                local_share: binding.map_or(LocalShareState::Inactive, |binding| {
                    LocalShareState::Active { share_id: binding.share_id, epoch: binding.epoch }
                }),
                remote_share: RemoteShareState::Inactive,
            }]),
        };
        let mut projection = MediaProjection::default();

        projection.apply(MediaEvent::LocalFrame {
            session_id,
            binding: binding_a,
            frame: test_frame(),
        });
        projection.reconcile_shares(&snapshot(Some(binding_a)));
        assert!(projection.session(session_id).local_frame.is_some());

        projection.reconcile_shares(&snapshot(Some(binding_b)));
        assert!(projection.session(session_id).local_frame.is_none());
        projection.apply(MediaEvent::LocalFrame {
            session_id,
            binding: binding_a,
            frame: test_frame(),
        });
        projection.reconcile_shares(&snapshot(Some(binding_b)));

        let session = projection.session(session_id);
        assert!(session.local_frame.is_none());
        assert_eq!(session.local_frame_binding, None);
    }

    #[tokio::test]
    async fn pipeline_shutdown_deadline_aborts_supervisor_and_children() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let child_dropped = Arc::new(AtomicBool::new(false));
        let flag = child_dropped.clone();
        let child = tokio::spawn(async move {
            let _flag = DropFlag(flag);
            std::future::pending::<Result<(), String>>().await
        });
        let child_abort = child.abort_handle();
        let supervisor_abort = child_abort.clone();
        let supervisor = tokio::spawn(async move {
            let _guard = ChildTaskGuard::new(vec![supervisor_abort]);
            let _ = child.await;
        });
        let mut pipeline =
            OwnedPipeline { stop: None, task: Some(supervisor), children: vec![child_abort] };
        tokio::task::yield_now().await;

        pipeline.shutdown_with_timeout(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;

        assert!(child_dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn future_epoch_parks_the_old_pipeline_until_reconciliation_replaces_it() {
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let parked = tokio::spawn(async move {
            park_until_pipeline_replacement(&mut cancel_rx).await;
        });
        tokio::task::yield_now().await;
        assert!(!parked.is_finished());

        cancel_tx.send_replace(true);
        tokio::time::timeout(Duration::from_secs(1), parked)
            .await
            .expect("parked pipeline did not observe replacement")
            .unwrap();
    }
}
