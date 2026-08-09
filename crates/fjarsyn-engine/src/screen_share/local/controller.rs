use std::{panic::AssertUnwindSafe, sync::Arc};

use futures::{FutureExt, StreamExt};
use tokio::sync::{RwLock, mpsc, watch};

use super::{
    super::{ChildTaskGuard, OwnedPipeline, task_failure},
    CaptureGuard, Plan, requires_capture_readback, stop_capture,
};
use crate::{
    media::{
        PixelFormat,
        capture::{PlatformItem, PlatformProvider, PlatformProviderBuilder, Provider},
        codec::{
            DirectionState, EncoderSessionParts, EncoderWorkerConfig,
            ServiceHandle as CodecServiceHandle,
        },
    },
    peer_session::{EncodedVideoSample, EncodedVideoSink, LocalShareState, SessionId},
    screen_share::{Config, LocalShareBinding, LocalState, Output, Update},
};

struct Reservation {
    selection: crate::screen_share::SelectionKey,
    phase: ReservationPhase,
}

enum ReservationPhase {
    Selecting,
    Starting { expires_at: tokio::time::Instant },
}

struct Pipeline {
    selection: crate::screen_share::SelectionKey,
    binding: LocalShareBinding,
    capture: Arc<RwLock<PlatformProvider>>,
    worker: OwnedPipeline,
    stop_requested: bool,
}

pub(in crate::screen_share) struct Controller {
    output: Output,
    codecs: CodecServiceHandle,
    restart_required: bool,
    reservation: Option<Reservation>,
    pending_stop: Option<LocalShareBinding>,
    pipeline: Option<Pipeline>,
}

impl Controller {
    pub(in crate::screen_share) fn new(output: Output, codecs: CodecServiceHandle) -> Self {
        Self {
            output,
            codecs,
            restart_required: false,
            reservation: None,
            pending_stop: None,
            pipeline: None,
        }
    }

    pub(in crate::screen_share) async fn begin_selection(
        &mut self,
        selection: crate::screen_share::SelectionKey,
    ) -> Result<(), String> {
        let session_id = selection.session_id();
        if selection.is_cancelled() {
            return Err("screen-share selection was cancelled".into());
        }
        if self.restart_required {
            let reason = "the video encoder is unavailable until Fjarsyn restarts".to_owned();
            self.emit(Update::LocalState { session_id, state: LocalState::Failed(reason.clone()) })
                .await;
            return Err(reason);
        }
        if let Some(pipeline) = &self.pipeline {
            return Err(format!(
                "screen sharing is already active for session {}",
                pipeline.binding.session_id
            ));
        }
        if self.pending_stop.is_some() {
            return Err("the previous screen share is still stopping".into());
        }
        if self.reservation.is_some() {
            return Err("another capture selection is already in progress".into());
        }
        self.reservation =
            Some(Reservation { selection: selection.clone(), phase: ReservationPhase::Selecting });
        self.emit(Update::LocalState { session_id, state: LocalState::Selecting }).await;
        Ok(())
    }

    pub(in crate::screen_share) async fn begin_start(
        &mut self,
        selection: &crate::screen_share::SelectionKey,
    ) -> Result<(), String> {
        let exact_selection = self
            .reservation
            .as_ref()
            .is_some_and(|reservation| reservation.selection == *selection);
        if !exact_selection {
            return Err("screen-share selection is no longer current".into());
        }
        if !matches!(
            self.reservation.as_ref().map(|reservation| &reservation.phase),
            Some(ReservationPhase::Selecting)
        ) {
            return Err("screen-share selection has already started".into());
        }
        if selection.is_cancelled() {
            self.cancel_selection(selection).await?;
            return Err("screen-share selection was cancelled".into());
        }
        if self.restart_required {
            let reason = "the video encoder is unavailable until Fjarsyn restarts".to_owned();
            self.fail_selection(selection, reason.clone()).await?;
            return Err(reason);
        }
        self.pending_stop = None;
        let reservation = self.reservation.as_mut().expect("exact reservation was validated");
        reservation.phase = ReservationPhase::Starting {
            expires_at: tokio::time::Instant::now() + super::super::START_OPERATION_TIMEOUT,
        };
        self.emit(Update::LocalState {
            session_id: selection.session_id(),
            state: LocalState::Starting,
        })
        .await;
        Ok(())
    }

    pub(in crate::screen_share) async fn cancel_selection(
        &mut self,
        selection: &crate::screen_share::SelectionKey,
    ) -> Result<(), String> {
        let Some(reservation) =
            self.reservation.as_ref().filter(|reservation| reservation.selection == *selection)
        else {
            return Err("screen-share selection is no longer current".into());
        };
        reservation.selection.cancel();
        self.reservation = None;
        self.emit(Update::LocalState {
            session_id: selection.session_id(),
            state: LocalState::Inactive,
        })
        .await;
        Ok(())
    }

    pub(in crate::screen_share) async fn fail_selection(
        &mut self,
        selection: &crate::screen_share::SelectionKey,
        reason: String,
    ) -> Result<(), String> {
        let Some(reservation) =
            self.reservation.as_ref().filter(|reservation| reservation.selection == *selection)
        else {
            return Err("screen-share selection is no longer current".into());
        };
        reservation.selection.cancel();
        self.reservation = None;
        self.emit(Update::LocalState {
            session_id: selection.session_id(),
            state: LocalState::Failed(reason),
        })
        .await;
        Ok(())
    }

    pub(in crate::screen_share) async fn start(
        &mut self,
        selection: &crate::screen_share::SelectionKey,
        item: PlatformItem,
        sink: EncodedVideoSink,
        config: Config,
    ) -> Result<(), String> {
        self.ensure_start_current(selection)?;
        let session_id = sink.session_id();
        let share_id = sink.share_id();
        if selection.session_id() != session_id {
            return Err("screen-share selection and encoded-video sink do not match".into());
        }
        if self.pipeline.as_ref().is_some_and(|pipeline| pipeline.worker.is_finished()) {
            let stale = self.pipeline.take().expect("finished local pipeline disappeared");
            let _ = stale.worker.shutdown().await;
            stop_capture(stale.capture);
        }
        if let Some(active) = &self.pipeline {
            return Err(format!(
                "screen sharing is already active for session {}",
                active.binding.session_id
            ));
        }

        let binding = LocalShareBinding::new(session_id, share_id, sink.epoch());
        self.emit(Update::LocalState { session_id, state: LocalState::Starting }).await;

        let provider = PlatformProviderBuilder::new(
            PixelFormat::DEFAULT_CAPTURE,
            config.capture.record_cursor,
            config.capture.recording_border_indicator,
            requires_capture_readback(&config),
        )
        .with_default_device()
        .and_then(|builder| builder.with_default_capture_item())
        .and_then(|builder| builder.build())
        .map_err(|error| error.to_string())?;
        let capture = CaptureGuard::new(Arc::new(RwLock::new(provider)));
        self.ensure_start_current(selection)?;

        {
            let mut provider = capture.capture().write().await;
            provider.set_capture_item(item).map_err(|error| error.to_string())?;
            provider.start_capture().map_err(|error| error.to_string())?;
        }
        self.ensure_start_current(selection)?;

        let mut stream =
            match capture.capture().write().await.create_stream(config.video.target_framerate) {
                Ok(stream) => stream,
                Err(error) => return Err(error.to_string()),
            };
        let codec_device = capture.capture().read().await.codec_device();
        self.ensure_start_current(selection)?;
        let encoder_config = EncoderWorkerConfig {
            bitrate: config.video.target_bitrate_bps,
            target_framerate_hz: config.video.target_framerate.to_hz(),
            target_resolution: config.video.target_resolution,
            device: codec_device,
            transcoding_type: config.video.transcoding_type,
        };
        let encoder = match tokio::select! {
            biased;
            _ = selection.cancelled() => {
                return Err("screen-share start was cancelled".into());
            }
            result = self.codecs.open_encoder(encoder_config) => result,
        } {
            Ok(encoder) => encoder,
            Err(error) => return Err(error.to_string()),
        };
        if let Err(error) = self.ensure_start_current(selection) {
            drop(encoder);
            return Err(error);
        }
        let EncoderSessionParts {
            input: encoder_input,
            output: mut encoder_output,
            worker: encoder_worker,
        } = encoder.into_parts();

        self.ensure_start_current(selection)?;
        let project_preview = config.capture.enable_ui_preview;
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let mut capture_cancel = cancel_rx.clone();
        let capture_cancel_tx = cancel_tx.clone();
        let capture_output = self.output.clone();
        let keyframe_requests = sink.clone();
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
                if project_preview {
                    capture_output.publish(Update::LocalFrame {
                        session_id,
                        binding: binding.media(),
                        frame: frame.clone(),
                    });
                }
                if keyframe_requests.take_keyframe_request() {
                    encoder_input.request_keyframe();
                }
                match encoder_input.try_send(frame) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                }
            }
        });

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
                        DirectionState::RestartRequired(_)
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

        let pipeline_output = self.output.clone();
        let failure_capture = capture.capture().clone();
        let child_aborts = vec![
            capture_task.abort_handle(),
            encoder_task.abort_handle(),
            network_task.abort_handle(),
        ];
        let supervisor_aborts = child_aborts.clone();
        let task = tokio::spawn(async move {
            let failure = AssertUnwindSafe(async move {
                let mut child_guard = ChildTaskGuard::new(supervisor_aborts);
                let (capture, encoder, network) =
                    tokio::join!(capture_task, encoder_task, network_task);
                child_guard.disarm();
                [
                    task_failure("capture", capture),
                    task_failure("encoder", encoder),
                    task_failure("video transport", network),
                ]
                .into_iter()
                .flatten()
                .next()
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Some("local media pipeline supervisor panicked".into()));
            let state = match failure {
                Some(reason) => {
                    stop_capture(failure_capture);
                    LocalState::Failed(reason)
                }
                None => LocalState::Inactive,
            };
            pipeline_output.publish(Update::LocalState { session_id: binding.session_id, state });
        });

        let capture = capture.disarm();
        self.pipeline = Some(Pipeline {
            selection: selection.clone(),
            binding,
            capture,
            worker: OwnedPipeline {
                stop: Some(cancel_tx),
                task: Some(task),
                children: child_aborts,
            },
            stop_requested: false,
        });
        self.reservation = None;
        self.emit(Update::LocalState { session_id, state: LocalState::Active }).await;
        Ok(())
    }

    pub(in crate::screen_share) async fn abort_start(
        &mut self,
        selection: &crate::screen_share::SelectionKey,
    ) -> Option<LocalShareBinding> {
        selection.cancel();
        if self.reservation.as_ref().is_some_and(|reservation| reservation.selection == *selection)
        {
            self.reservation = None;
            self.emit(Update::LocalState {
                session_id: selection.session_id(),
                state: LocalState::Inactive,
            })
            .await;
            return None;
        }

        let pipeline =
            self.pipeline.as_mut().filter(|pipeline| pipeline.selection == *selection)?;
        pipeline.stop_requested = true;
        pipeline.worker.request_stop();
        stop_capture(pipeline.capture.clone());
        let binding = pipeline.binding;
        self.pending_stop = Some(binding);
        self.emit(Update::LocalState {
            session_id: selection.session_id(),
            state: LocalState::Stopping,
        })
        .await;
        Some(binding)
    }

    pub(in crate::screen_share) async fn request_stop(
        &mut self,
        session_id: SessionId,
        snapshot: &crate::peer_session::Sessions,
    ) -> Option<LocalShareBinding> {
        if self
            .reservation
            .as_ref()
            .is_some_and(|reservation| reservation.selection.session_id() == session_id)
            && let Some(reservation) = self.reservation.take()
        {
            reservation.selection.cancel();
        }
        let binding = if let Some(pipeline) =
            self.pipeline.as_mut().filter(|pipeline| pipeline.binding.session_id == session_id)
        {
            self.pending_stop = None;
            pipeline.stop_requested = true;
            pipeline.worker.request_stop();
            stop_capture(pipeline.capture.clone());
            Some(pipeline.binding)
        } else {
            snapshot.session(session_id).and_then(|session| match session.local_share {
                LocalShareState::Active { share_id, epoch } => {
                    Some(LocalShareBinding::new(session_id, share_id, epoch))
                }
                LocalShareState::Inactive => None,
            })
        };
        if self.pipeline.is_none() {
            self.pending_stop = binding;
        }
        let state = if binding.is_some() { LocalState::Stopping } else { LocalState::Inactive };
        self.emit(Update::LocalState { session_id, state }).await;
        binding
    }

    pub(in crate::screen_share) fn has_state_for(&self, session_id: SessionId) -> bool {
        self.reservation
            .as_ref()
            .is_some_and(|reservation| reservation.selection.session_id() == session_id)
            || self.pending_stop.is_some_and(|binding| binding.session_id == session_id)
            || self
                .pipeline
                .as_ref()
                .is_some_and(|pipeline| pipeline.binding.session_id == session_id)
    }

    pub(in crate::screen_share) async fn report_failure(
        &self,
        session_id: SessionId,
        reason: String,
    ) {
        self.emit(Update::LocalState { session_id, state: LocalState::Failed(reason) }).await;
    }

    pub(in crate::screen_share) async fn reconciliation(
        &mut self,
        snapshot: &crate::peer_session::Sessions,
    ) -> Plan {
        let terminal_reservation = self.reservation.as_ref().and_then(|reservation| {
            if reservation.selection.is_cancelled()
                || !snapshot
                    .session(reservation.selection.session_id())
                    .is_some_and(super::super::permits_local_share_start)
            {
                Some(LocalState::Inactive)
            } else if matches!(
                reservation.phase,
                ReservationPhase::Starting { expires_at }
                    if expires_at <= tokio::time::Instant::now()
            ) {
                Some(LocalState::Failed("screen-share startup did not complete in time".into()))
            } else {
                None
            }
        });
        if let Some(state) = terminal_reservation
            && let Some(reservation) = self.reservation.take()
        {
            reservation.selection.cancel();
            self.emit(Update::LocalState { session_id: reservation.selection.session_id(), state })
                .await;
        }

        let active_shares = snapshot
            .sessions
            .iter()
            .filter(|session| super::super::retains_media_session(session.phase))
            .filter_map(|session| match session.local_share {
                LocalShareState::Active { share_id, epoch } => {
                    Some(LocalShareBinding::new(session.session_id, share_id, epoch))
                }
                LocalShareState::Inactive => None,
            })
            .collect::<Vec<_>>();
        let pipeline = self.pipeline.as_ref().map(|pipeline| {
            (
                pipeline.binding,
                pipeline.stop_requested
                    || pipeline.worker.is_finished()
                    || pipeline.selection.is_cancelled(),
            )
        });
        let pending_session = self.reservation.as_ref().and_then(|reservation| {
            matches!(reservation.phase, ReservationPhase::Starting { .. })
                .then(|| reservation.selection.session_id())
        });
        let plan = Plan::reconcile(&active_shares, pipeline, pending_session, self.pending_stop);
        if plan.confirmed_stop.is_some_and(|binding| self.pending_stop == Some(binding))
            && let Some(binding) = self.pending_stop.take()
        {
            self.emit(Update::LocalState {
                session_id: binding.session_id,
                state: LocalState::Inactive,
            })
            .await;
        }
        plan
    }

    pub(in crate::screen_share) async fn teardown(&mut self, binding: LocalShareBinding) {
        let Some(pipeline) = self.pipeline.take() else {
            return;
        };
        if pipeline.binding != binding {
            self.pipeline = Some(pipeline);
            return;
        }
        let _ = pipeline.worker.shutdown().await;
        stop_capture(pipeline.capture);
        self.emit(Update::LocalState {
            session_id: binding.session_id,
            state: LocalState::Inactive,
        })
        .await;
    }

    pub(in crate::screen_share) async fn stop_session(&mut self, session_id: SessionId) {
        if self.pending_stop.is_some_and(|binding| binding.session_id == session_id) {
            self.pending_stop = None;
        }
        if self
            .reservation
            .as_ref()
            .is_some_and(|reservation| reservation.selection.session_id() == session_id)
            && let Some(reservation) = self.reservation.take()
        {
            reservation.selection.cancel();
        }
        if let Some(binding) = self
            .pipeline
            .as_ref()
            .filter(|pipeline| pipeline.binding.session_id == session_id)
            .map(|pipeline| pipeline.binding)
        {
            self.teardown(binding).await;
        } else {
            self.emit(Update::LocalState { session_id, state: LocalState::Inactive }).await;
        }
    }

    pub(in crate::screen_share) fn require_restart(&mut self) -> bool {
        if self.restart_required {
            return false;
        }
        self.restart_required = true;
        if let Some(reservation) = self.reservation.take() {
            reservation.selection.cancel();
        }
        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.stop_requested = true;
            pipeline.worker.request_stop();
            stop_capture(pipeline.capture.clone());
        }
        true
    }

    pub(in crate::screen_share) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> bool {
        if let Some(reservation) = self.reservation.take() {
            reservation.selection.cancel();
        }
        self.pending_stop = None;
        let Some(Pipeline { mut worker, capture, .. }) = self.pipeline.take() else {
            return true;
        };
        worker.request_stop();
        let clean = worker.shutdown_until(deadline).await;
        stop_capture(capture);
        clean
    }

    fn cancel_now(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            reservation.selection.cancel();
        }
        self.pending_stop = None;
        if let Some(pipeline) = self.pipeline.take() {
            stop_capture(pipeline.capture);
            drop(pipeline.worker);
        }
    }

    async fn emit(&self, update: Update) {
        self.output.publish(update);
    }

    fn ensure_start_current(
        &self,
        selection: &crate::screen_share::SelectionKey,
    ) -> Result<(), String> {
        if self.restart_required {
            return Err("the video encoder is unavailable until Fjarsyn restarts".into());
        }
        if selection.is_cancelled() {
            return Err("screen-share start was cancelled".into());
        }
        let Some(reservation) =
            self.reservation.as_ref().filter(|reservation| reservation.selection == *selection)
        else {
            return Err("screen-share selection is no longer current".into());
        };
        match reservation.phase {
            ReservationPhase::Starting { expires_at }
                if expires_at > tokio::time::Instant::now() => {}
            ReservationPhase::Starting { .. } => {
                return Err("screen-share startup did not complete in time".into());
            }
            ReservationPhase::Selecting => {
                return Err("screen-share selection has not started".into());
            }
        }
        Ok(())
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel_now();
    }
}
