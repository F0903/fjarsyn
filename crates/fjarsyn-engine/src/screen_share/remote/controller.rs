use std::{collections::BTreeMap, panic::AssertUnwindSafe};

use futures::FutureExt;
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    super::{ChildTaskGuard, OwnedPipeline, task_failure},
    Pipeline, contains_nal_type,
};
use crate::{
    media::{
        PixelFormat,
        codec::{
            DecoderSessionParts, DecoderWorkerConfig, DirectionState,
            ServiceHandle as CodecServiceHandle,
        },
    },
    peer_session::{self, RemoteVideoRead, RemoteVideoSource, SessionId},
    screen_share::{Config, Output, RemoteState, ShareBinding, Update},
};

pub(in crate::screen_share) struct Controller {
    output: Output,
    codecs: CodecServiceHandle,
    restart_required: bool,
    standby: BTreeMap<SessionId, RemoteVideoSource>,
    pipelines: BTreeMap<SessionId, Pipeline>,
}

impl Controller {
    pub(in crate::screen_share) fn new(output: Output, codecs: CodecServiceHandle) -> Self {
        Self {
            output,
            codecs,
            restart_required: false,
            standby: BTreeMap::new(),
            pipelines: BTreeMap::new(),
        }
    }

    pub(in crate::screen_share) async fn start(
        &mut self,
        session_id: SessionId,
        binding: ShareBinding,
        config: Config,
    ) -> Result<(), String> {
        if self.restart_required {
            return Err("the video decoder is unavailable until Fjarsyn restarts".into());
        }
        if self.is_running(session_id, binding) {
            return Ok(());
        }
        if let Some(stale) = self.pipelines.remove(&session_id)
            && let Some(source) = stale.shutdown().await
        {
            self.standby.insert(session_id, source);
        }
        let Some(mut source) = self.standby.remove(&session_id) else {
            let reason = "remote video standby source is unavailable".to_owned();
            self.emit(Update::RemoteState {
                session_id,
                state: RemoteState::Failed(reason.clone()),
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
                self.standby.insert(session_id, source);
                let reason = error.to_string();
                self.emit(Update::RemoteState {
                    session_id,
                    state: RemoteState::Failed(reason.clone()),
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
        self.emit(Update::RemoteState { session_id, state: RemoteState::Starting }).await;
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
                            Pipeline::park_until_replacement(&mut source_cancel).await;
                            break Ok(());
                        }
                        Err(peer_session::Error::RemoteVideoLagged { skipped }) => {
                            tracing::debug!(
                                %session_id,
                                skipped,
                                "remote video source lagged; continuing at the retained media boundary"
                            );
                            continue;
                        }
                        Err(error) => {
                            let _ = source_cancel_tx.send(true);
                            break Err(error.to_string());
                        }
                    },
                };
                if !found_sequence_parameter_set {
                    found_sequence_parameter_set = contains_nal_type(&sample.data, 7);
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

        let mut output_cancel = cancel_rx;
        let frame_output = self.output.clone();
        let decoder_health = self.codecs.clone();
        let frame_output_task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = output_cancel.changed() => return Ok::<(), String>(()),
                    frame = decoder_output.recv() => match frame {
                        Some(Ok(frame)) => frame,
                        Some(Err(error)) => {
                            return Err(format!("failed to decode remote video: {error}"));
                        }
                        None => return Ok(()),
                    },
                };
                if matches!(decoder_health.snapshot().decode, DirectionState::RestartRequired(_)) {
                    return Ok(());
                }
                frame_output.publish(Update::RemoteFrame { session_id, binding, frame });
            }
        });

        let pipeline_output = self.output.clone();
        let child_aborts = vec![
            source_task.abort_handle(),
            decoder_task.abort_handle(),
            frame_output_task.abort_handle(),
        ];
        let supervisor_aborts = child_aborts.clone();
        let task = tokio::spawn(async move {
            let failure = AssertUnwindSafe(async move {
                let mut child_guard = ChildTaskGuard::new(supervisor_aborts);
                let (source, decoder, frame_output) =
                    tokio::join!(source_task, decoder_task, frame_output_task);
                child_guard.disarm();
                [
                    task_failure("remote video source", source),
                    task_failure("decoder", decoder),
                    task_failure("decoded-frame output", frame_output),
                ]
                .into_iter()
                .flatten()
                .next()
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Some("remote media pipeline supervisor panicked".into()));
            let state = failure.map(RemoteState::Failed).unwrap_or(RemoteState::Inactive);
            pipeline_output.publish(Update::RemoteState { session_id, state });
        });

        self.pipelines.insert(
            session_id,
            Pipeline {
                binding,
                worker: OwnedPipeline {
                    stop: Some(cancel_tx),
                    task: Some(task),
                    children: child_aborts,
                },
                source_return,
            },
        );
        self.emit(Update::RemoteState { session_id, state: RemoteState::Active }).await;
        Ok(())
    }

    pub(in crate::screen_share) fn receiver_ready(&self, session_id: SessionId) -> bool {
        self.restart_required
            || self.standby.contains_key(&session_id)
            || self.pipelines.contains_key(&session_id)
    }

    pub(in crate::screen_share) fn restart_required(&self) -> bool {
        self.restart_required
    }

    pub(in crate::screen_share) fn install_standby(
        &mut self,
        session_id: SessionId,
        source: RemoteVideoSource,
    ) {
        if !self.restart_required && !self.receiver_ready(session_id) {
            self.standby.insert(session_id, source);
        }
    }

    pub(in crate::screen_share) fn is_running(
        &self,
        session_id: SessionId,
        binding: ShareBinding,
    ) -> bool {
        self.pipelines
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.binding == binding && !pipeline.worker.is_finished())
    }

    pub(in crate::screen_share) async fn stop_session(&mut self, session_id: SessionId) {
        self.stop(session_id).await;
        self.standby.remove(&session_id);
    }

    pub(in crate::screen_share) async fn stop(&mut self, session_id: SessionId) {
        if let Some(pipeline) = self.pipelines.remove(&session_id)
            && let Some(source) = pipeline.shutdown().await
        {
            self.standby.insert(session_id, source);
        }
        self.emit(Update::RemoteState { session_id, state: RemoteState::Inactive }).await;
    }

    pub(in crate::screen_share) fn require_restart(&mut self) -> bool {
        if self.restart_required {
            return false;
        }
        self.restart_required = true;
        self.standby.clear();
        let mut pipelines = std::mem::take(&mut self.pipelines);
        for pipeline in pipelines.values_mut() {
            pipeline.worker.request_stop();
        }
        drop(pipelines);
        true
    }

    pub(in crate::screen_share) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> bool {
        let mut pipelines = std::mem::take(&mut self.pipelines);
        for pipeline in pipelines.values_mut() {
            pipeline.worker.request_stop();
        }
        let clean = futures::future::join_all(
            pipelines.values_mut().map(|pipeline| pipeline.worker.shutdown_until(deadline)),
        )
        .await
        .into_iter()
        .all(|clean| clean);
        self.standby.clear();
        clean
    }

    fn cancel_now(&mut self) {
        self.standby.clear();
        self.pipelines.clear();
    }

    async fn emit(&self, update: Update) {
        self.output.publish(update);
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel_now();
    }
}
