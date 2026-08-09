//! Encoder channel construction and async watchdog state machine.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::{mpsc, oneshot, watch};

use super::{
    Command, EncodedFrame, EncoderInput, EncoderOutput, EncoderSession, EncoderWorkerConfig, Thread,
};
use crate::media::{
    codec::{
        Direction, Error, Operation, PoisonReason, State, Worker, WorkerError,
        registry::{WorkerDirective, WorkerReservationParts},
        worker::WorkerCompletion,
    },
    frame::Frame,
};

const ENCODER_INPUT_CAPACITY: usize = 2;
const ENCODER_OUTPUT_CAPACITY: usize = 3;

pub(in crate::media::codec) struct Supervisor {
    directive: watch::Receiver<WorkerDirective>,
    input: mpsc::Receiver<Arc<Frame>>,
    output: mpsc::Sender<EncodedFrame>,
    thread: Thread,
    publishing: Arc<AtomicBool>,
    keyframe_requested: Arc<AtomicBool>,
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
    Frame(Arc<Frame>),
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

enum CallResult {
    Reply(Result<Vec<Vec<u8>>, String>),
    Timeout,
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

enum PublishResult {
    Sent,
    Stop,
    Terminated,
    Directive(WorkerDirective),
}

impl Supervisor {
    pub(in crate::media::codec) async fn start(
        state: Arc<State>,
        config: EncoderWorkerConfig,
    ) -> Result<EncoderSession, Error> {
        let WorkerReservationParts { id, directive, accepting, publishing } =
            state.reserve_worker(Direction::Encode)?.into_parts();
        let (input_tx, input) = mpsc::channel(ENCODER_INPUT_CAPACITY);
        let (output, output_rx) = mpsc::channel(ENCODER_OUTPUT_CAPACITY);
        let keyframe_requested = Arc::new(AtomicBool::new(false));
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
        let supervisor = Self {
            directive,
            input,
            output,
            thread,
            publishing: publishing.clone(),
            keyframe_requested: keyframe_requested.clone(),
            started,
        };
        state.spawn_supervisor(id, Direction::Encode, supervisor.run()).await;

        started_rx.await.unwrap_or(Err(Error::ShuttingDown))?;
        Ok(EncoderSession::new(
            EncoderInput::new(input_tx, accepting, keyframe_requested),
            EncoderOutput::new(output_rx, completion_rx.clone(), publishing),
            Worker::new(id, &state, completion_rx),
        ))
    }

    async fn run(self) {
        let Self {
            mut directive,
            mut input,
            output,
            thread,
            publishing,
            keyframe_requested,
            started,
        } = self;
        let (thread_commands, mut ready, thread_lifecycle) = thread.into_components();
        let mut commands = Some(thread_commands);
        let mut lifecycle = Some(thread_lifecycle);
        let state = lifecycle.as_ref().expect("encoder lifecycle present").state().clone();

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
            _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                StartupResult::Terminated
            }
        };

        match startup {
            StartupResult::Ready => {
                if started.send(Ok(())).is_err() {
                    drop(commands.take());
                    lifecycle.take().expect("encoder lifecycle present").finish(Ok(()), true).await;
                    return;
                }
            }
            StartupResult::CodecError(error) => {
                let _ = started.send(Err(Error::Codec(error.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(WorkerError::Codec(error)), true)
                    .await;
                return;
            }
            StartupResult::Timeout => {
                let poison = state.poison(
                    Direction::Encode,
                    Operation::Initialize,
                    PoisonReason::DeadlineExceeded,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Terminated => {
                let poison = state.poison(
                    Direction::Encode,
                    Operation::Initialize,
                    PoisonReason::WorkerTerminated,
                );
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Directive(WorkerDirective::Poisoned(poison)) => {
                let _ = started.send(Err(Error::RestartRequired(poison.clone())));
                drop(commands.take());
                lifecycle
                    .take()
                    .expect("encoder lifecycle present")
                    .finish(Err(WorkerError::RestartRequired(poison)), false)
                    .await;
                return;
            }
            StartupResult::Directive(_) => {
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
                    Ok(()) => LoopEvent::Directive(directive.borrow().clone()),
                    Err(_) => LoopEvent::Directive(WorkerDirective::ServiceShutdown),
                },
                _ = output.closed() => LoopEvent::Stop,
                frame = input.recv() => match frame {
                    Some(frame) => LoopEvent::Frame(frame),
                    None => LoopEvent::Stop,
                },
                _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                    LoopEvent::Terminated
                }
            };

            let frame = match next {
                LoopEvent::Frame(frame) => frame,
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
                        Direction::Encode,
                        Operation::Encode,
                        PoisonReason::WorkerTerminated,
                    );
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
            };

            let Some(duration) = frame.duration else {
                continue;
            };
            let force_keyframe = keyframe_requested.swap(false, Ordering::AcqRel);
            let (reply, mut response) = oneshot::channel();
            let command = Command::Encode { frame, force_keyframe, reply };
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
                _ = lifecycle.as_ref().expect("encoder lifecycle present").wait_until_thread_finished() => {
                    CallResult::Terminated
                }
            };
            match call {
                CallResult::Reply(Ok(nal_units)) => {
                    if !nal_units.is_empty() && publishing.load(Ordering::Acquire) {
                        let encoded_frame = EncodedFrame { nal_units, duration };
                        let publish = loop {
                            let reservation = tokio::select! {
                                biased;
                                changed = directive.changed() => {
                                    let directive = if changed.is_ok() {
                                        directive.borrow().clone()
                                    } else {
                                        WorkerDirective::ServiceShutdown
                                    };
                                    Err(PublishResult::Directive(directive))
                                },
                                result = output.reserve() => {
                                    result.map_err(|_| PublishResult::Stop)
                                },
                                _ = lifecycle
                                    .as_ref()
                                    .expect("encoder lifecycle present")
                                    .wait_until_thread_finished() => {
                                        Err(PublishResult::Terminated)
                                    },
                            };
                            match reservation {
                                Ok(permit) => {
                                    permit.send(encoded_frame);
                                    break PublishResult::Sent;
                                }
                                Err(PublishResult::Directive(WorkerDirective::Run)) => continue,
                                Err(result) => break result,
                            }
                        };
                        match publish {
                            PublishResult::Sent => {}
                            PublishResult::Stop
                            | PublishResult::Directive(WorkerDirective::Stop)
                            | PublishResult::Directive(WorkerDirective::ServiceShutdown) => {
                                break (Ok(()), true);
                            }
                            PublishResult::Directive(WorkerDirective::Poisoned(poison)) => {
                                break (Err(WorkerError::RestartRequired(poison)), false);
                            }
                            PublishResult::Directive(WorkerDirective::Run) => {
                                unreachable!("run directives are retried while publishing")
                            }
                            PublishResult::Terminated => {
                                let poison = state.poison(
                                    Direction::Encode,
                                    Operation::Encode,
                                    PoisonReason::WorkerTerminated,
                                );
                                break (Err(WorkerError::RestartRequired(poison)), false);
                            }
                        }
                    }
                }
                CallResult::Reply(Err(error)) => {
                    break (Err(WorkerError::Codec(error)), true);
                }
                CallResult::Timeout => {
                    let poison = state.poison(
                        Direction::Encode,
                        Operation::Encode,
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
                        Direction::Encode,
                        Operation::Encode,
                        PoisonReason::WorkerTerminated,
                    );
                    break (Err(WorkerError::RestartRequired(poison)), false);
                }
            }
        };

        drop(commands);
        lifecycle.take().expect("encoder lifecycle present").finish(terminal.0, terminal.1).await;
    }
}
