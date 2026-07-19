//! Native encoder thread and its command/startup channels.

use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::{mpsc, oneshot, watch};

use super::EncoderWorkerConfig;
use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecDirection, State,
        registry::WorkerId,
        worker::{CodecWorkerApartment, CodecWorkerLifecycle, WorkerCompletion},
    },
};

pub(in crate::services::codec_service) enum EncoderCommand {
    Encode { frame: Arc<Frame>, reply: oneshot::Sender<Result<Vec<Vec<u8>>, String>> },
}

pub(in crate::services::codec_service) struct EncoderThread {
    commands: mpsc::Sender<EncoderCommand>,
    ready: oneshot::Receiver<Result<(), String>>,
    lifecycle: CodecWorkerLifecycle,
}

pub(in crate::services::codec_service) struct EncoderThreadParts {
    pub(in crate::services::codec_service) commands: mpsc::Sender<EncoderCommand>,
    pub(in crate::services::codec_service) ready: oneshot::Receiver<Result<(), String>>,
    pub(in crate::services::codec_service) lifecycle: CodecWorkerLifecycle,
}

impl EncoderThread {
    pub(in crate::services::codec_service) fn spawn(
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
        let lifecycle = CodecWorkerLifecycle::spawn(
            state,
            id,
            CodecDirection::Encode,
            format!("fjarsyn-codec-encode-{id}"),
            completion,
            accepting,
            publishing,
            move || {
                let _apartment = match CodecWorkerApartment::initialize() {
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
                while let Some(EncoderCommand::Encode { frame, reply }) = command_rx.blocking_recv()
                {
                    let result = encoder.encode(&frame);
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

    pub(in crate::services::codec_service) fn into_parts(self) -> EncoderThreadParts {
        EncoderThreadParts { commands: self.commands, ready: self.ready, lifecycle: self.lifecycle }
    }
}
