//! Decoder channel construction and async watchdog state machine.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    DecoderCommand, DecoderInput, DecoderOutput, DecoderSession, DecoderThread, DecoderWorkerConfig,
};
use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecDirection, CodecOperation, CodecPoisonReason, CodecWorkerError, Error, State,
        registry::{WorkerDirective, WorkerReservationParts},
        worker::{CodecWorker, WorkerCompletion},
    },
};

const DECODER_INPUT_CAPACITY: usize = 8;
const DECODER_OUTPUT_CAPACITY: usize = 2;

pub(in crate::services::codec_service) struct DecoderSupervisor {
    directive: watch::Receiver<WorkerDirective>,
    input: mpsc::Receiver<Bytes>,
    output: mpsc::Sender<Arc<Frame>>,
    thread: DecoderThread,
    publishing: Arc<AtomicBool>,
    started: oneshot::Sender<Result<(), Error>>,
}

enum DecoderStartupResult {
    Ready,
    CodecError(String),
    Timeout,
    Terminated,
    Directive(WorkerDirective),
}

enum DecoderLoopEvent {
    Packet(Bytes),
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

enum DecoderCallResult {
    Reply(Result<Option<Arc<Frame>>, String>),
    Timeout,
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

impl DecoderSupervisor {
    pub(in crate::services::codec_service) async fn start(
        state: Arc<State>,
        config: DecoderWorkerConfig,
    ) -> Result<DecoderSession, Error> {
        let WorkerReservationParts { id, directive, accepting, publishing } =
            state.reserve_worker(CodecDirection::Decode)?.into_parts();
        let (input_tx, input) = mpsc::channel(DECODER_INPUT_CAPACITY);
        let (output, output_rx) = mpsc::channel(DECODER_OUTPUT_CAPACITY);
        let (completion_tx, completion_rx) = watch::channel(WorkerCompletion::Running);
        let thread = match DecoderThread::spawn(
            state.clone(),
            id,
            config,
            completion_tx,
            accepting.clone(),
            publishing.clone(),
        ) {
            Ok(thread) => thread,
            Err(error) => {
                state.remove_worker(id);
                return Err(Error::WorkerSpawn(error.to_string()));
            }
        };

        let (started, started_rx) = oneshot::channel();
        tokio::spawn(
            Self { directive, input, output, thread, publishing: publishing.clone(), started }
                .run(),
        );

        started_rx.await.unwrap_or(Err(Error::ShuttingDown))?;
        Ok(DecoderSession::new(
            DecoderInput::new(input_tx, accepting),
            DecoderOutput::new(output_rx, completion_rx.clone(), publishing),
            CodecWorker::new(id, &state, completion_rx),
        ))
    }

    async fn run(self) {
        let Self { mut directive, mut input, output, thread, publishing, started } = self;
        let thread = thread.into_parts();
        let mut commands = Some(thread.commands);
        let mut ready = thread.ready;
        let mut lifecycle = Some(thread.lifecycle);
        let state = lifecycle.as_ref().expect("decoder lifecycle present").state().clone();

        let startup = tokio::select! {
            biased;
            changed = directive.changed() => match changed {
                Ok(()) => DecoderStartupResult::Directive(directive.borrow().clone()),
                Err(_) => DecoderStartupResult::Directive(WorkerDirective::ServiceShutdown),
            },
            result = &mut ready => match result {
                Ok(Ok(())) => DecoderStartupResult::Ready,
                Ok(Err(error)) => DecoderStartupResult::CodecError(error),
                Err(_) => DecoderStartupResult::Terminated,
            },
            _ = tokio::time::sleep(state.call_timeout()) => DecoderStartupResult::Timeout,
            _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                DecoderStartupResult::Terminated
            }
        };

        match startup {
            DecoderStartupResult::Ready => {
                if started.send(Ok(())).is_err() {
                    drop(commands.take());
                    lifecycle.take().expect("decoder lifecycle present").finish(Ok(()), true).await;
                    return;
                }
            }
            DecoderStartupResult::CodecError(error) => {
                let _ = started.send(Err(Error::Codec(error.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(CodecWorkerError::Codec(error)), true)
                    .await;
                return;
            }
            DecoderStartupResult::Timeout => {
                let poison = state.poison(
                    CodecDirection::Decode,
                    CodecOperation::Initialize,
                    CodecPoisonReason::DeadlineExceeded,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            DecoderStartupResult::Terminated => {
                let poison = state.poison(
                    CodecDirection::Decode,
                    CodecOperation::Initialize,
                    CodecPoisonReason::WorkerTerminated,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            DecoderStartupResult::Directive(WorkerDirective::Poisoned(poison)) => {
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            DecoderStartupResult::Directive(_) => {
                let _ = started.send(Err(Error::ShuttingDown));
                drop(commands.take());
                lifecycle.take().expect("decoder lifecycle present").finish(Ok(()), true).await;
                return;
            }
        }

        let commands = commands.take().expect("decoder commands present");
        let terminal = loop {
            let next = tokio::select! {
                biased;
                changed = directive.changed() => match changed {
                    Ok(()) => DecoderLoopEvent::Directive(directive.borrow().clone()),
                    Err(_) => DecoderLoopEvent::Directive(WorkerDirective::ServiceShutdown),
                },
                _ = output.closed() => DecoderLoopEvent::Stop,
                packet = input.recv() => match packet {
                    Some(packet) => DecoderLoopEvent::Packet(packet),
                    None => DecoderLoopEvent::Stop,
                },
                _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                    DecoderLoopEvent::Terminated
                }
            };

            let packet = match next {
                DecoderLoopEvent::Packet(packet) => packet,
                DecoderLoopEvent::Stop | DecoderLoopEvent::Directive(WorkerDirective::Stop) => {
                    break (Ok(()), true);
                }
                DecoderLoopEvent::Directive(WorkerDirective::ServiceShutdown) => {
                    break (Ok(()), true);
                }
                DecoderLoopEvent::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                DecoderLoopEvent::Directive(WorkerDirective::Run) => continue,
                DecoderLoopEvent::Terminated => {
                    let poison = state.poison(
                        CodecDirection::Decode,
                        CodecOperation::Decode,
                        CodecPoisonReason::WorkerTerminated,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
            };

            let (reply, mut response) = oneshot::channel();
            let command = DecoderCommand::Decode { packet, reply };
            let sent = tokio::select! {
                biased;
                changed = directive.changed() => {
                    let directive = if changed.is_ok() {
                        directive.borrow().clone()
                    } else {
                        WorkerDirective::ServiceShutdown
                    };
                    Err(directive)
                },
                result = commands.send(command) => result.map_err(|_| WorkerDirective::Stop),
            };
            if let Err(stop) = sent {
                match stop {
                    WorkerDirective::Poisoned(poison) => {
                        break (Err(CodecWorkerError::RestartRequired(poison)), false);
                    }
                    _ => break (Ok(()), true),
                }
            }

            let call = tokio::select! {
                biased;
                changed = directive.changed() => {
                    let directive = if changed.is_ok() {
                        directive.borrow().clone()
                    } else {
                        WorkerDirective::ServiceShutdown
                    };
                    DecoderCallResult::Directive(directive)
                },
                result = &mut response => match result {
                    Ok(result) => DecoderCallResult::Reply(result),
                    Err(_) => DecoderCallResult::Terminated,
                },
                _ = tokio::time::sleep(state.call_timeout()) => DecoderCallResult::Timeout,
                _ = output.closed() => DecoderCallResult::Stop,
                _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                    DecoderCallResult::Terminated
                }
            };
            match call {
                DecoderCallResult::Reply(Ok(Some(frame))) => {
                    if publishing.load(Ordering::Acquire) {
                        match output.try_send(frame) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break (Ok(()), true),
                        }
                    }
                }
                DecoderCallResult::Reply(Ok(None)) => {}
                DecoderCallResult::Reply(Err(error)) => {
                    break (Err(CodecWorkerError::Codec(error)), true);
                }
                DecoderCallResult::Timeout => {
                    let poison = state.poison(
                        CodecDirection::Decode,
                        CodecOperation::Decode,
                        CodecPoisonReason::DeadlineExceeded,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                DecoderCallResult::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                DecoderCallResult::Directive(_) | DecoderCallResult::Stop => break (Ok(()), true),
                DecoderCallResult::Terminated => {
                    let poison = state.poison(
                        CodecDirection::Decode,
                        CodecOperation::Decode,
                        CodecPoisonReason::WorkerTerminated,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
            }
        };

        drop(commands);
        lifecycle.take().expect("decoder lifecycle present").finish(terminal.0, terminal.1).await;
    }
}
