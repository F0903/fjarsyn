//! Native encoder thread and its command/startup channels.

use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::{mpsc, oneshot, watch};

use super::EncoderWorkerConfig;
use crate::media::{
    codec::{
        Direction, State,
        registry::WorkerId,
        worker::{WorkerApartment, WorkerCompletion, WorkerLifecycle},
    },
    frame::Frame,
};

pub(in crate::media::codec) enum Command {
    Encode {
        frame: Arc<Frame>,
        force_keyframe: bool,
        reply: oneshot::Sender<Result<Vec<Vec<u8>>, String>>,
    },
}

pub(in crate::media::codec) struct Thread {
    commands: mpsc::Sender<Command>,
    ready: oneshot::Receiver<Result<(), String>>,
    lifecycle: WorkerLifecycle,
}

impl Thread {
    pub(in crate::media::codec) fn spawn(
        state: Arc<State>,
        id: WorkerId,
        config: EncoderWorkerConfig,
        completion: watch::Sender<WorkerCompletion>,
        accepting: Arc<AtomicBool>,
        publishing: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let (commands, mut command_rx) = mpsc::channel(1);
        let (ready_tx, ready) = oneshot::channel();
        let backend = state.backend();
        let lifecycle = WorkerLifecycle::spawn(
            state,
            id,
            Direction::Encode,
            completion,
            accepting,
            publishing,
            move || {
                let _apartment = match WorkerApartment::initialize() {
                    Ok(apartment) => apartment,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        return Err(error);
                    }
                };
                let mut encoder = match backend.create_encoder(config) {
                    Ok(encoder) => {
                        let _ = ready_tx.send(Ok(()));
                        encoder
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        return Err(error);
                    }
                };
                while let Some(Command::Encode { frame, force_keyframe, reply }) =
                    command_rx.blocking_recv()
                {
                    let result = encoder.encode(&frame, force_keyframe);
                    let terminal = result.as_ref().err().cloned();
                    let _ = reply.send(result);
                    if let Some(error) = terminal {
                        return Err(error);
                    }
                }
                drop(encoder);
                Ok(())
            },
        )?;
        Ok(Self { commands, ready, lifecycle })
    }

    pub(in crate::media::codec) fn into_components(
        self,
    ) -> (mpsc::Sender<Command>, oneshot::Receiver<Result<(), String>>, WorkerLifecycle) {
        (self.commands, self.ready, self.lifecycle)
    }
}
