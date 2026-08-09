use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use tokio::sync::{oneshot, watch};

use super::h264::DecoderBootstrapGate;
use crate::{
    media::codec::{
        DecoderSession, DecoderWorkerConfig, DirectionState, ServiceHandle as CodecServiceHandle,
    },
    peer_session::{RemoteVideoRead, RemoteVideoSource, SessionId},
    screen_share::{
        Output, PIPELINE_SHUTDOWN_TIMEOUT, RemoteState, ShareBinding, Update,
        owned_pipeline::OwnedPipeline,
    },
};

pub(super) struct Pipeline {
    pub(super) binding: ShareBinding,
    pub(super) worker: OwnedPipeline,
    pub(super) source_return: oneshot::Receiver<RemoteVideoSource>,
}

impl Pipeline {
    pub(super) fn spawn(
        session_id: SessionId,
        binding: ShareBinding,
        mut source: RemoteVideoSource,
        decoder: DecoderSession,
        decoder_config: DecoderWorkerConfig,
        codecs: CodecServiceHandle,
        output: Output,
    ) -> Self {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (source_return_tx, source_return) = oneshot::channel();
        let pipeline_output = output.clone();
        let task = tokio::spawn(async move {
            let result = AssertUnwindSafe(run(
                session_id,
                binding,
                &mut source,
                decoder,
                decoder_config,
                codecs,
                output,
                cancel_rx,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err("remote media pipeline panicked".into()));
            let _ = source_return_tx.send(source);
            let state = result.map_or_else(RemoteState::Failed, |()| RemoteState::Inactive);
            pipeline_output.publish(Update::RemoteState { session_id, state });
        });

        Self {
            binding,
            worker: OwnedPipeline { stop: Some(cancel_tx), task: Some(task), children: Vec::new() },
            source_return,
        }
    }

    pub(super) async fn shutdown(mut self) -> Option<RemoteVideoSource> {
        self.worker.request_stop();
        let (_, source) = tokio::join!(
            self.worker.shutdown(),
            tokio::time::timeout(PIPELINE_SHUTDOWN_TIMEOUT, self.source_return)
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
}

enum Next {
    Source(Result<RemoteVideoRead, crate::peer_session::Error>),
    Decoded(
        Option<
            Result<std::sync::Arc<crate::media::frame::Frame>, crate::media::codec::WorkerError>,
        >,
    ),
}

#[allow(clippy::too_many_arguments)]
async fn run(
    session_id: SessionId,
    binding: ShareBinding,
    source: &mut RemoteVideoSource,
    mut decoder: DecoderSession,
    decoder_config: DecoderWorkerConfig,
    codecs: CodecServiceHandle,
    output: Output,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    let mut gate = DecoderBootstrapGate::default();

    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.changed() => return Ok(()),
            sample = source.recv_for(binding.epoch) => Next::Source(sample),
            frame = decoder.recv() => Next::Decoded(frame),
        };

        match next {
            Next::Source(Ok(RemoteVideoRead::EpochAdvanced { next_epoch })) => {
                tracing::debug!(
                    %session_id,
                    share_epoch = binding.epoch.value(),
                    next_share_epoch = next_epoch.value(),
                    "remote media advanced before its control event was projected; parking the old decoder"
                );
                decoder.request_stop();
                let _ = cancel.changed().await;
                return Ok(());
            }
            Next::Source(Ok(RemoteVideoRead::Sample(sample))) => {
                if sample.starts_after_discontinuity() {
                    tracing::debug!(
                        %session_id,
                        share_epoch = binding.epoch.value(),
                        "remote video became discontinuous; replacing the decoder and awaiting an SPS/PPS/IDR boundary"
                    );
                    if gate.is_synchronized() {
                        decoder = recreate_decoder(decoder, decoder_config, &codecs).await?;
                    }
                    gate.reset();
                }

                let was_synchronized = gate.is_synchronized();
                let accepted = match gate.accepts(&sample.data) {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::debug!(
                            %session_id,
                            share_epoch = binding.epoch.value(),
                            %error,
                            "rejecting malformed remote H.264 access unit"
                        );
                        if was_synchronized {
                            decoder = recreate_decoder(decoder, decoder_config, &codecs).await?;
                        }
                        gate.reset();
                        continue;
                    }
                };

                if !accepted {
                    continue;
                }

                let sent = tokio::select! {
                    biased;
                    _ = cancel.changed() => return Ok(()),
                    sent = decoder.send(sample.data) => sent,
                };
                if sent.is_err() {
                    return Err("remote decoder stopped accepting media".into());
                }

                // The source branch is intentionally preferred so a queued
                // discontinuity can stop stale publication before a decoded
                // frame wins the select. Under a sustained source backlog,
                // however, that preference must not starve the sole decoder
                // output consumer. Drain every result that is ready now;
                // decoder output is bounded and no new input is submitted
                // during this loop.
                while let Some(decoded) = decoder.recv().now_or_never() {
                    if !handle_decoded(decoded, session_id, binding, &codecs, &output)? {
                        return Ok(());
                    }
                }
            }
            Next::Source(Err(error)) => return Err(error.to_string()),
            Next::Decoded(decoded) => {
                if !handle_decoded(decoded, session_id, binding, &codecs, &output)? {
                    return Ok(());
                }
            }
        }
    }
}

fn handle_decoded(
    decoded: Option<
        Result<std::sync::Arc<crate::media::frame::Frame>, crate::media::codec::WorkerError>,
    >,
    session_id: SessionId,
    binding: ShareBinding,
    codecs: &CodecServiceHandle,
    output: &Output,
) -> Result<bool, String> {
    match decoded {
        Some(Ok(frame)) => {
            if matches!(codecs.snapshot().decode, DirectionState::RestartRequired(_)) {
                return Ok(false);
            }
            output.publish(Update::RemoteFrame { session_id, binding, frame });
            Ok(true)
        }
        Some(Err(error)) => Err(format!("failed to decode remote video: {error}")),
        None => Ok(false),
    }
}

async fn recreate_decoder(
    stale: DecoderSession,
    config: DecoderWorkerConfig,
    codecs: &CodecServiceHandle,
) -> Result<DecoderSession, String> {
    // This release-store disables publication before shutdown waits for the
    // native decoder, so no frame from the broken reference chain can escape.
    stale.request_stop();
    stale.shutdown().await.map_err(|error| format!("failed to reset remote decoder: {error}"))?;
    codecs
        .open_decoder(config)
        .await
        .map_err(|error| format!("failed to reopen remote decoder: {error}"))
}
