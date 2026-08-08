//! Native decoder thread and its command/startup channels.

use std::sync::{Arc, atomic::AtomicBool};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, watch};

use super::DecoderWorkerConfig;
use crate::media::{
    codec::{
        Direction, State,
        registry::WorkerId,
        worker::{WorkerApartment, WorkerCompletion, WorkerLifecycle},
    },
    frame::Frame,
};

pub(in crate::media::codec) enum Command {
    Decode { packet: Bytes, reply: oneshot::Sender<Result<Option<Arc<Frame>>, String>> },
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
        config: DecoderWorkerConfig,
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
            Direction::Decode,
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
                let mut decoder = match backend.create_decoder(config) {
                    Ok(decoder) => {
                        let _ = ready_tx.send(Ok(()));
                        decoder
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        return Err(error);
                    }
                };
                while let Some(Command::Decode { packet, reply }) = command_rx.blocking_recv() {
                    let result = decoder.decode(&packet);
                    let terminal = result.as_ref().err().cloned();
                    let _ = reply.send(result);
                    if let Some(error) = terminal {
                        return Err(error);
                    }
                }
                drop(decoder);
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
