use std::{collections::BTreeMap, sync::Arc, time::Duration};

use fjarsyn_core::{
    capture_providers::{CaptureProvider, PlatformCaptureItem, PlatformCaptureProvider},
    config::Config,
    media::{
        ffmpeg::{FFmpegDecoder, FFmpegEncoder, FFmpegTranscodeTypeExt, HWAccelType},
        frame::Frame,
        pixel_format::PixelFormat,
    },
    peer_session::{
        EncodedVideoSample, EncodedVideoSink, LocalShareState, PeerSessionServiceHandle,
        PeerSessionServiceSnapshot, RemoteShareState, RemoteVideoSource, SessionId, ShareId,
    },
};
use futures::StreamExt;
use tokio::{
    sync::{RwLock, mpsc, oneshot, watch},
    task::{AbortHandle, JoinHandle},
};

const PIPELINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

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

#[derive(Debug, Clone, Default)]
pub struct MediaSessionProjection {
    pub local: LocalMediaState,
    pub remote: RemoteMediaState,
    pub local_frame: Option<Arc<Frame>>,
    pub remote_frame: Option<Arc<Frame>>,
    pub remote_frame_share_id: Option<ShareId>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaProjection {
    pub sessions: Arc<BTreeMap<SessionId, MediaSessionProjection>>,
}

impl MediaProjection {
    pub fn session(&self, session_id: SessionId) -> MediaSessionProjection {
        self.sessions.get(&session_id).cloned().unwrap_or_default()
    }

    pub fn apply(&mut self, event: MediaEvent) {
        let sessions = Arc::make_mut(&mut self.sessions);
        match event {
            MediaEvent::LocalState { session_id, state } => {
                let projection = sessions.entry(session_id).or_default();
                projection.local = state;
                if matches!(
                    projection.local,
                    LocalMediaState::Inactive
                        | LocalMediaState::Stopping
                        | LocalMediaState::Failed(_)
                ) {
                    projection.local_frame = None;
                }
            }
            MediaEvent::RemoteState { session_id, state } => {
                let projection = sessions.entry(session_id).or_default();
                projection.remote = state;
                if matches!(
                    projection.remote,
                    RemoteMediaState::Inactive | RemoteMediaState::Failed(_)
                ) {
                    projection.remote_frame = None;
                    projection.remote_frame_share_id = None;
                }
            }
            MediaEvent::LocalFrame { session_id, frame } => {
                sessions.entry(session_id).or_default().local_frame = Some(frame);
            }
            MediaEvent::RemoteFrame { session_id, share_id, frame } => {
                let projection = sessions.entry(session_id).or_default();
                projection.remote_frame = Some(frame);
                projection.remote_frame_share_id = Some(share_id);
            }
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
            let active_share_id =
                snapshot.session(*session_id).and_then(|session| match session.remote_share {
                    RemoteShareState::Active { share_id } => Some(share_id),
                    RemoteShareState::Inactive => None,
                });
            if projection.remote_frame_share_id != active_share_id {
                projection.remote_frame = None;
                projection.remote_frame_share_id = None;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MediaEvent {
    LocalState { session_id: SessionId, state: LocalMediaState },
    RemoteState { session_id: SessionId, state: RemoteMediaState },
    LocalFrame { session_id: SessionId, frame: Arc<Frame> },
    RemoteFrame { session_id: SessionId, share_id: ShareId, frame: Arc<Frame> },
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
        self.request_stop();
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(timeout, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if error.is_cancelled() => {}
                Ok(Err(error)) => tracing::warn!("media pipeline task failed: {error}"),
                Err(_) => {
                    tracing::warn!(
                        "media pipeline exceeded its shutdown deadline; aborting async workers; \
                         an in-progress FFmpeg call will finish cooperatively"
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
    share_id: ShareId,
    worker: OwnedPipeline,
    source_return: oneshot::Receiver<RemoteVideoSource>,
}

pub struct SessionMediaService {
    event_tx: mpsc::Sender<super::RuntimeEvent>,
    sessions: PeerSessionServiceHandle,
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
    ) -> Self {
        Self {
            event_tx,
            sessions,
            pending_local_start: None,
            pending_local_stop: None,
            local: None,
            standby_remote: BTreeMap::new(),
            remote: BTreeMap::new(),
        }
    }

    pub async fn mark_selecting(&self, session_id: SessionId) {
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Selecting }).await;
    }

    pub async fn begin_local_start(&mut self, session_id: SessionId) {
        self.pending_local_stop = None;
        self.pending_local_start = Some(PendingLocalStart {
            session_id,
            expires_at: tokio::time::Instant::now() + Duration::from_secs(30),
        });
        self.emit(MediaEvent::LocalState { session_id, state: LocalMediaState::Starting }).await;
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
        session_id: SessionId,
        share_id: ShareId,
        item: PlatformCaptureItem,
        sink: EncodedVideoSink,
        config: Config,
    ) -> Result<(), String> {
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

        let binding = LocalShareBinding { session_id, share_id };

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
        let device_handle =
            if config.video.transcoding_type.get_encoder_info().hw_accel == HWAccelType::D3D11VA {
                capture.read().await.raw_device_handle()
            } else {
                None
            };
        let mut encoder = match FFmpegEncoder::new(
            config.video.target_bitrate,
            config.video.target_framerate.to_hz(),
            config.video.target_resolution,
            PixelFormat::DEFAULT_CAPTURE,
            device_handle,
            config.video.transcoding_type,
        ) {
            Ok(encoder) => encoder,
            Err(error) => {
                let _ = capture.write().await.stop_capture();
                return Err(error.to_string());
            }
        };

        let project_local_preview = config.capture.enable_ui_preview;
        let transcode = config.video.transcoding_type;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (frame_tx, mut frame_rx) = mpsc::channel::<Arc<Frame>>(2);
        let (encoded_tx, mut encoded_rx) = mpsc::channel::<EncodedVideoSample>(3);

        let mut capture_cancel = cancel_rx.clone();
        let capture_cancel_tx = cancel_tx.clone();
        let capture_events = self.event_tx.clone();
        let capture_task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
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
                        MediaEvent::LocalFrame { session_id, frame: frame.clone() },
                    ));
                }
                match frame_tx.try_send(frame) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                }
            }
        });

        // FFmpeg is deliberately isolated from the async executor. Capture and
        // network tasks remain responsive while the blocking worker performs one
        // synchronous codec call at a time. Tokio cannot abort a call already in
        // progress; shutdown closes its bounded input and lets that call finish.
        let encoder_cancel_tx = cancel_tx.clone();
        let encoder_task = tokio::task::spawn_blocking(move || {
            let result = (|| {
                while let Some(frame) = frame_rx.blocking_recv() {
                    let Some(duration) = frame.duration else {
                        continue;
                    };
                    let nal_units =
                        encoder
                            .encode(&frame, transcode, frame.size.x, frame.size.y)
                            .map_err(|error| format!("failed to encode screen frame: {error}"))?;
                    for nal in nal_units {
                        match encoded_tx.try_send(EncodedVideoSample::new(nal, duration)) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => break,
                            Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                        }
                    }
                }
                Ok(())
            })();
            let _ = encoder_cancel_tx.send(true);
            result
        });

        let mut network_cancel = cancel_rx;
        let network_cancel_tx = cancel_tx.clone();
        let network_task = tokio::spawn(async move {
            loop {
                let sample = tokio::select! {
                    _ = network_cancel.changed() => return Ok::<(), String>(()),
                    sample = encoded_rx.recv() => match sample {
                        Some(sample) => sample,
                        None => return Ok(()),
                    },
                };
                tokio::select! {
                    _ = network_cancel.changed() => return Ok(()),
                    result = sink.send(sample) => {
                        if let Err(error) = result {
                            let _ = network_cancel_tx.send(true);
                            return Err(format!("video transport closed: {error}"));
                        }
                    }
                }
            }
        });

        let pipeline_events = self.event_tx.clone();
        let failure_capture = capture.clone();
        let child_aborts = vec![capture_task.abort_handle(), network_task.abort_handle()];
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
                    LocalShareState::Active { share_id } => {
                        Some(LocalShareBinding { session_id, share_id })
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
                LocalShareState::Active { share_id } => {
                    Some(LocalShareBinding { session_id: session.session_id, share_id })
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
        share_id: ShareId,
        config: Config,
    ) -> Result<(), String> {
        if self
            .remote
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.share_id == share_id && !pipeline.worker.is_finished())
        {
            return Ok(());
        }
        if let Some(stale) = self.remote.remove(&session_id)
            && let Some(source) = shutdown_remote_pipeline(stale).await
        {
            self.standby_remote.insert(session_id, source);
        }
        let Some(mut source) = self.standby_remote.remove(&session_id) else {
            return Err("remote video standby source is unavailable".into());
        };
        self.emit(MediaEvent::RemoteState { session_id, state: RemoteMediaState::Starting }).await;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (packet_tx, mut packet_rx) = mpsc::channel::<bytes::Bytes>(8);
        let (frame_tx, mut frame_rx) = mpsc::channel::<Arc<Frame>>(2);
        let (source_return_tx, source_return) = oneshot::channel();

        let mut source_cancel = cancel_rx.clone();
        let source_cancel_tx = cancel_tx.clone();
        let source_task = tokio::spawn(async move {
            let result = loop {
                let sample = tokio::select! {
                    _ = source_cancel.changed() => break Ok::<(), String>(()),
                    sample = source.recv() => match sample {
                        Ok(sample) => sample,
                        Err(error) => {
                            let _ = source_cancel_tx.send(true);
                            break Err(error.to_string());
                        }
                    },
                };
                match packet_tx.try_send(sample.data) {
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
        let transcode = config.video.transcoding_type;
        let decoder_task = tokio::task::spawn_blocking(move || {
            let result = (|| {
                let mut decoder = FFmpegDecoder::new(transcode, PixelFormat::DEFAULT_CAPTURE)
                    .map_err(|error| format!("failed to create video decoder: {error}"))?;
                let mut found_sequence_parameter_set = false;
                while let Some(packet) = packet_rx.blocking_recv() {
                    // Each native decoder belongs to one authenticated ShareId.
                    // Ignore any tail buffered from the prior share until the new
                    // encoder's SPS arrives, then decode its complete fresh GOP.
                    if !found_sequence_parameter_set {
                        found_sequence_parameter_set = contains_h264_nal_type(&packet, 7);
                        if !found_sequence_parameter_set {
                            continue;
                        }
                    }
                    if let Some(frame) = decoder
                        .decode(&packet)
                        .map_err(|error| format!("failed to decode remote video: {error}"))?
                    {
                        match frame_tx.try_send(frame) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                        }
                    }
                }
                Ok(())
            })();
            let _ = decoder_cancel_tx.send(true);
            result
        });

        let mut projection_cancel = cancel_rx;
        let projection_events = self.event_tx.clone();
        let projection_task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = projection_cancel.changed() => return Ok::<(), String>(()),
                    frame = frame_rx.recv() => match frame {
                        Some(frame) => frame,
                        None => return Ok(()),
                    },
                };
                let _ = projection_events.try_send(super::RuntimeEvent::Media(
                    MediaEvent::RemoteFrame { session_id, share_id, frame },
                ));
            }
        });

        let pipeline_events = self.event_tx.clone();
        let child_aborts = vec![source_task.abort_handle(), projection_task.abort_handle()];
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
                share_id,
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
        self.standby_remote.contains_key(&session_id) || self.remote.contains_key(&session_id)
    }

    pub fn install_standby_remote(&mut self, session_id: SessionId, source: RemoteVideoSource) {
        if !self.remote_receiver_ready(session_id) {
            self.standby_remote.insert(session_id, source);
        }
    }

    pub fn remote_is_running(&self, session_id: SessionId, share_id: ShareId) -> bool {
        self.remote
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.share_id == share_id && !pipeline.worker.is_finished())
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

    pub async fn shutdown(&mut self) {
        self.pending_local_start = None;
        self.pending_local_stop = None;
        if let Some(local) = self.local.take() {
            local.worker.shutdown().await;
            let _ = local.capture.write().await.stop_capture();
        }
        let remote = std::mem::take(&mut self.remote);
        futures::future::join_all(remote.into_values().map(|pipeline| pipeline.worker.shutdown()))
            .await;
        self.standby_remote.clear();
    }

    async fn emit(&self, event: MediaEvent) {
        let _ = self.event_tx.send(super::RuntimeEvent::Media(event)).await;
    }

    pub(super) fn cancel_now(&mut self) {
        self.pending_local_start = None;
        self.pending_local_stop = None;
        if let Some(local) = self.local.take() {
            if let Ok(mut capture) = local.capture.try_write() {
                let _ = capture.stop_capture();
            }
            drop(local.worker);
        }
        self.standby_remote.clear();
        self.remote.clear();
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

    #[test]
    fn local_pipeline_cleanup_keeps_the_original_share_identity() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding = LocalShareBinding { session_id, share_id };

        assert_eq!(binding.session_id, session_id);
        assert_eq!(binding.share_id, share_id);
    }

    #[test]
    fn local_reconciliation_supervises_stop_until_exact_share_is_inactive() {
        let binding = LocalShareBinding { session_id: SessionId::new(), share_id: ShareId::new() };

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
        let binding = LocalShareBinding { session_id: SessionId::new(), share_id: ShareId::new() };

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
        let pending = LocalShareBinding { session_id: pending_session, share_id: ShareId::new() };
        let orphan = LocalShareBinding { session_id: SessionId::new(), share_id: ShareId::new() };

        let plan = plan_local_reconciliation(&[pending, orphan], None, Some(pending_session), None);

        assert_eq!(plan.stop_core, vec![orphan]);
    }

    #[test]
    fn changed_share_id_never_reuses_an_obsolete_native_pipeline() {
        let session_id = SessionId::new();
        let native_binding = LocalShareBinding { session_id, share_id: ShareId::new() };
        let core_binding = LocalShareBinding { session_id, share_id: ShareId::new() };

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
        let binding = LocalShareBinding { session_id: SessionId::new(), share_id: ShareId::new() };

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
    fn stopped_share_rejects_a_late_queued_remote_frame() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let snapshot = |remote_share| PeerSessionServiceSnapshot {
            sessions: Arc::new(vec![PeerSessionSnapshot {
                session_id,
                peer_id: PeerId::new("peer-a").unwrap(),
                phase: PeerSessionPhase::Connected,
                local_share: LocalShareState::Inactive,
                remote_share,
            }]),
        };
        let frame = || {
            Arc::new(Frame {
                data: FrameData::Software(bytes::Bytes::new()),
                format: PixelFormat::BGRA8,
                size: Vector2::new(0, 0),
                duration: None,
            })
        };
        let active = snapshot(RemoteShareState::Active { share_id });
        let stopped = snapshot(RemoteShareState::Inactive);
        let mut projection = MediaProjection::default();

        projection.apply(MediaEvent::RemoteFrame { session_id, share_id, frame: frame() });
        projection.reconcile_shares(&active);
        assert!(projection.session(session_id).remote_frame.is_some());

        projection.reconcile_shares(&stopped);
        projection.apply(MediaEvent::RemoteFrame { session_id, share_id, frame: frame() });
        projection.reconcile_shares(&stopped);

        let session = projection.session(session_id);
        assert!(session.remote_frame.is_none());
        assert_eq!(session.remote_frame_share_id, None);
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
}
