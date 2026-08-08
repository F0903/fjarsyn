//! Decoder channel construction and async watchdog state machine.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, watch};

use super::{Command, DecoderInput, DecoderOutput, DecoderSession, DecoderWorkerConfig, Thread};
use crate::media::{
    codec::{
        Direction, Error, Operation, PoisonReason, State, Worker, WorkerError,
        registry::{WorkerDirective, WorkerReservationParts},
        worker::WorkerCompletion,
    },
    frame::Frame,
};

const DECODER_INPUT_CAPACITY: usize = 8;
const DECODER_OUTPUT_CAPACITY: usize = 2;

pub(in crate::media::codec) struct Supervisor {
    directive: watch::Receiver<WorkerDirective>,
    input: mpsc::Receiver<Bytes>,
    output: mpsc::Sender<Arc<Frame>>,
    thread: Thread,
    publishing: Arc<AtomicBool>,
    started: oneshot::Sender<Result<(), Error>>,
}

enum StartupResult {
    Ready,
    CodecError(String),
    Timeout,
    Terminated,
    Directive(WorkerDirective),
}

enum LoopEvent {
    Packet(Bytes),
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

enum CallResult {
    Reply(Result<Option<Arc<Frame>>, String>),
    Timeout,
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

impl Supervisor {
    pub(in crate::media::codec) async fn start(
        state: Arc<State>,
        config: DecoderWorkerConfig,
    ) -> Result<DecoderSession, Error> {
        let WorkerReservationParts { id, directive, accepting, publishing } =
            state.reserve_worker(Direction::Decode)?.into_parts();
        let (input_tx, input) = mpsc::channel(DECODER_INPUT_CAPACITY);
        let (output, output_rx) = mpsc::channel(DECODER_OUTPUT_CAPACITY);
        let (completion_tx, completion_rx) = watch::channel(WorkerCompletion::Running);
        let thread = match Thread::spawn(
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
        let supervisor =
            Self { directive, input, output, thread, publishing: publishing.clone(), started };
        state.spawn_supervisor(id, Direction::Decode, supervisor.run()).await;

        started_rx.await.unwrap_or(Err(Error::ShuttingDown))?;
        Ok(DecoderSession::new(
            DecoderInput::new(input_tx, accepting),
            DecoderOutput::new(output_rx, completion_rx.clone(), publishing),
            Worker::new(id, &state, completion_rx),
        ))
    }

    async fn run(self) {
        let Self { mut directive, mut input, output, thread, publishing, started } = self;
        let (thread_commands, mut ready, thread_lifecycle) = thread.into_components();
        let mut commands = Some(thread_commands);
        let mut lifecycle = Some(thread_lifecycle);
        let state = lifecycle.as_ref().expect("decoder lifecycle present").state().clone();

        let startup = tokio::select! {
            biased;
            changed = directive.changed() => match changed {
                Ok(()) => StartupResult::Directive(directive.borrow().clone()),
                Err(_) => StartupResult::Directive(WorkerDirective::ServiceShutdown),
            },
            result = &mut ready => match result {
                Ok(Ok(())) => StartupResult::Ready,
                Ok(Err(error)) => StartupResult::CodecError(error),
                Err(_) => StartupResult::Terminated,
            },
            _ = tokio::time::sleep(state.call_timeout()) => StartupResult::Timeout,
            _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                StartupResult::Terminated
            }
        };

        match startup {
            StartupResult::Ready => {
                if started.send(Ok(())).is_err() {
                    drop(commands.take());
                    lifecycle.take().expect("decoder lifecycle present").finish(Ok(()), true).await;
                    return;
                }
            }
            StartupResult::CodecError(error) => {
                let _ = started.send(Err(Error::Codec(error.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(WorkerError::Codec(error)), true)
                    .await;
                return;
            }
            StartupResult::Timeout => {
                let poison = state.poison(
                    Direction::Decode,
                    Operation::Initialize,
                    PoisonReason::DeadlineExceeded,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Terminated => {
                let poison = state.poison(
                    Direction::Decode,
                    Operation::Initialize,
                    PoisonReason::WorkerTerminated,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Directive(WorkerDirective::Poisoned(poison)) => {
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("decoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Directive(_) => {
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
                    Ok(()) => LoopEvent::Directive(directive.borrow().clone()),
                    Err(_) => LoopEvent::Directive(WorkerDirective::ServiceShutdown),
                },
                _ = output.closed() => LoopEvent::Stop,
                packet = input.recv() => match packet {
                    Some(packet) => LoopEvent::Packet(packet),
                    None => LoopEvent::Stop,
                },
                _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                    LoopEvent::Terminated
                }
            };

            let packet = match next {
                LoopEvent::Packet(packet) => packet,
                LoopEvent::Stop | LoopEvent::Directive(WorkerDirective::Stop) => {
                    break (Ok(()), true);
                }
                LoopEvent::Directive(WorkerDirective::ServiceShutdown) => {
                    break (Ok(()), true);
                }
                LoopEvent::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
                LoopEvent::Directive(WorkerDirective::Run) => continue,
                LoopEvent::Terminated => {
                    let poison = state.poison(
                        Direction::Decode,
                        Operation::Decode,
                        PoisonReason::WorkerTerminated,
                    );
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
            };

            let (reply, mut response) = oneshot::channel();
            let command = Command::Decode { packet, reply };
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
                        break (Err(WorkerError::RestartRequired(poison)), false);
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
                    CallResult::Directive(directive)
                },
                result = &mut response => match result {
                    Ok(result) => CallResult::Reply(result),
                    Err(_) => CallResult::Terminated,
                },
                _ = tokio::time::sleep(state.call_timeout()) => CallResult::Timeout,
                _ = output.closed() => CallResult::Stop,
                _ = lifecycle.as_ref().expect("decoder lifecycle present").wait_until_thread_finished() => {
                    CallResult::Terminated
                }
            };
            match call {
                CallResult::Reply(Ok(Some(frame))) => {
                    if publishing.load(Ordering::Acquire) {
                        match output.try_send(frame) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break (Ok(()), true),
                        }
                    }
                }
                CallResult::Reply(Ok(None)) => {}
                CallResult::Reply(Err(error)) => {
                    break (Err(WorkerError::Codec(error)), true);
                }
                CallResult::Timeout => {
                    let poison = state.poison(
                        Direction::Decode,
                        Operation::Decode,
                        PoisonReason::DeadlineExceeded,
                    );
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
                CallResult::Directive(WorkerDirective::Poisoned(poison)) => {
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
                CallResult::Directive(_) | CallResult::Stop => break (Ok(()), true),
                CallResult::Terminated => {
                    let poison = state.poison(
                        Direction::Decode,
                        Operation::Decode,
                        PoisonReason::WorkerTerminated,
                    );
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
            }
        };

        drop(commands);
        lifecycle.take().expect("decoder lifecycle present").finish(terminal.0, terminal.1).await;
    }
}
