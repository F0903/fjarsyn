//! Encoder channel construction and async watchdog state machine.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::{mpsc, oneshot, watch};

use super::{
    EncodedFrame, EncoderCommand, EncoderInput, EncoderOutput, EncoderSession, EncoderThread,
    EncoderWorkerConfig,
};
use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecDirection, CodecOperation, CodecPoisonReason, CodecWorkerError, Error, State,
        registry::{WorkerDirective, WorkerReservationParts},
        worker::{CodecWorker, WorkerCompletion},
    },
};

const ENCODER_INPUT_CAPACITY: usize = 2;
const ENCODER_OUTPUT_CAPACITY: usize = 3;

pub(in crate::services::codec_service) struct EncoderSupervisor {
    directive: watch::Receiver<WorkerDirective>,
    input: mpsc::Receiver<Arc<Frame>>,
    output: mpsc::Sender<EncodedFrame>,
    thread: EncoderThread,
    publishing: Arc<AtomicBool>,
    started: oneshot::Sender<Result<(), Error>>,
}

enum EncoderStartupResult {
    Ready,
    CodecError(String),
    Timeout,
    Terminated,
    Directive(WorkerDirective),
}

enum EncoderLoopEvent {
    Frame(Arc<Frame>),
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

enum EncoderCallResult {
    Reply(Result<Vec<Vec<u8>>, String>),
    Timeout,
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

impl EncoderSupervisor {
    pub(in crate::services::codec_service) async fn start(
        state: Arc<State>,
        config: EncoderWorkerConfig,
    ) -> Result<EncoderSession, Error> {
        let WorkerReservationParts { id, directive, accepting, publishing } =
            state.reserve_worker(CodecDirection::Encode)?.into_parts();
        let (input_tx, input) = mpsc::channel(ENCODER_INPUT_CAPACITY);
        let (output, output_rx) = mpsc::channel(ENCODER_OUTPUT_CAPACITY);
        let (completion_tx, completion_rx) = watch::channel(WorkerCompletion::Running);
        let thread = match EncoderThread::spawn(
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
        Ok(EncoderSession::new(
            EncoderInput::new(input_tx, accepting),
            EncoderOutput::new(output_rx, completion_rx.clone(), publishing),
            CodecWorker::new(id, &state, completion_rx),
        ))
    }

    async fn run(self) {
        let Self { mut directive, mut input, output, thread, publishing, started } = self;
        let thread = thread.into_parts();
        let mut commands = Some(thread.commands);
        let mut ready = thread.ready;
        let mut lifecycle = Some(thread.lifecycle);
        let state = lifecycle.as_ref().expect("encoder lifecycle present").state().clone();

        let startup = tokio::select! {
            biased;
            changed = directive.changed() => match changed {
                Ok(()) => EncoderStartupResult::Directive(directive.borrow().clone()),
                Err(_) => EncoderStartupResult::Directive(WorkerDirective::ServiceShutdown),
            },
            result = &mut ready => match result {
                Ok(Ok(())) => EncoderStartupResult::Ready,
                Ok(Err(error)) => EncoderStartupResult::CodecError(error),
                Err(_) => EncoderStartupResult::Terminated,
            },
            _ = tokio::time::sleep(state.call_timeout()) => EncoderStartupResult::Timeout,
            _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                EncoderStartupResult::Terminated
            }
        };

        match startup {
            EncoderStartupResult::Ready => {
                if started.send(Ok(())).is_err() {
                    drop(commands.take());
                    lifecycle.take().expect("encoder lifecycle present").finish(Ok(()), true).await;
                    return;
                }
            }
            EncoderStartupResult::CodecError(error) => {
                let _ = started.send(Err(Error::Codec(error.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(CodecWorkerError::Codec(error)), true)
                    .await;
                return;
            }
            EncoderStartupResult::Timeout => {
                let poison = state.poison(
                    CodecDirection::Encode,
                    CodecOperation::Initialize,
                    CodecPoisonReason::DeadlineExceeded,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            EncoderStartupResult::Terminated => {
                let poison = state.poison(
                    CodecDirection::Encode,
                    CodecOperation::Initialize,
                    CodecPoisonReason::WorkerTerminated,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            EncoderStartupResult::Directive(WorkerDirective::Poisoned(poison)) => {
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(CodecWorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            EncoderStartupResult::Directive(_) => {
                let _ = started.send(Err(Error::ShuttingDown));
                drop(commands.take());
                lifecycle.take().expect("encoder lifecycle present").finish(Ok(()), true).await;
                return;
            }
        }

        let commands = commands.take().expect("encoder commands present");
        let terminal = loop {
            let next = tokio::select! {
                biased;
                changed = directive.changed() => match changed {
                    Ok(()) => EncoderLoopEvent::Directive(directive.borrow().clone()),
                    Err(_) => EncoderLoopEvent::Directive(WorkerDirective::ServiceShutdown),
                },
                _ = output.closed() => EncoderLoopEvent::Stop,
                frame = input.recv() => match frame {
                    Some(frame) => EncoderLoopEvent::Frame(frame),
                    None => EncoderLoopEvent::Stop,
                },
                _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                    EncoderLoopEvent::Terminated
                }
            };

            let frame = match next {
                EncoderLoopEvent::Frame(frame) => frame,
                EncoderLoopEvent::Stop | EncoderLoopEvent::Directive(WorkerDirective::Stop) => {
                    break (Ok(()), true);
                }
                EncoderLoopEvent::Directive(WorkerDirective::ServiceShutdown) => {
                    break (Ok(()), true);
                }
                EncoderLoopEvent::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                EncoderLoopEvent::Directive(WorkerDirective::Run) => continue,
                EncoderLoopEvent::Terminated => {
                    let poison = state.poison(
                        CodecDirection::Encode,
                        CodecOperation::Encode,
                        CodecPoisonReason::WorkerTerminated,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
            };

            let Some(duration) = frame.duration else {
                continue;
            };
            let (reply, mut response) = oneshot::channel();
            let command = EncoderCommand::Encode { frame, reply };
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
                    EncoderCallResult::Directive(directive)
                },
                result = &mut response => match result {
                    Ok(result) => EncoderCallResult::Reply(result),
                    Err(_) => EncoderCallResult::Terminated,
                },
                _ = tokio::time::sleep(state.call_timeout()) => EncoderCallResult::Timeout,
                _ = output.closed() => EncoderCallResult::Stop,
                _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                    EncoderCallResult::Terminated
                }
            };
            match call {
                EncoderCallResult::Reply(Ok(nal_units)) => {
                    if !nal_units.is_empty() && publishing.load(Ordering::Acquire) {
                        match output.try_send(EncodedFrame { nal_units, duration }) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break (Ok(()), true),
                        }
                    }
                }
                EncoderCallResult::Reply(Err(error)) => {
                    break (Err(CodecWorkerError::Codec(error)), true);
                }
                EncoderCallResult::Timeout => {
                    let poison = state.poison(
                        CodecDirection::Encode,
                        CodecOperation::Encode,
                        CodecPoisonReason::DeadlineExceeded,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                EncoderCallResult::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
                EncoderCallResult::Directive(_) | EncoderCallResult::Stop => break (Ok(()), true),
                EncoderCallResult::Terminated => {
                    let poison = state.poison(
                        CodecDirection::Encode,
                        CodecOperation::Encode,
                        CodecPoisonReason::WorkerTerminated,
                    );
                    break (Err(CodecWorkerError::RestartRequired(poison)), false);
                }
            }
        };

        drop(commands);
        lifecycle.take().expect("encoder lifecycle present").finish(terminal.0, terminal.1).await;
    }
}
