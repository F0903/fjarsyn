//! Native decoder thread and its command/startup channels.

use std::sync::{Arc, atomic::AtomicBool};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, watch};

use super::DecoderWorkerConfig;
use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecDirection, State,
        registry::WorkerId,
        worker::{CodecWorkerApartment, CodecWorkerLifecycle, WorkerCompletion},
    },
};

pub(in crate::services::codec_service) enum DecoderCommand {
    Decode { packet: Bytes, reply: oneshot::Sender<Result<Option<Arc<Frame>>, String>> },
}

pub(in crate::services::codec_service) struct DecoderThread {
    commands: mpsc::Sender<DecoderCommand>,
    ready: oneshot::Receiver<Result<(), String>>,
    lifecycle: CodecWorkerLifecycle,
}

pub(in crate::services::codec_service) struct DecoderThreadParts {
    pub(in crate::services::codec_service) commands: mpsc::Sender<DecoderCommand>,
    pub(in crate::services::codec_service) ready: oneshot::Receiver<Result<(), String>>,
    pub(in crate::services::codec_service) lifecycle: CodecWorkerLifecycle,
}

impl DecoderThread {
    pub(in crate::services::codec_service) fn spawn(
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
        let lifecycle = CodecWorkerLifecycle::spawn(
            state,
            id,
            CodecDirection::Decode,
            format!("fjarsyn-codec-decode-{id}"),
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
                while let Some(DecoderCommand::Decode { packet, reply }) =
                    command_rx.blocking_recv()
                {
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

    pub(in crate::services::codec_service) fn into_parts(self) -> DecoderThreadParts {
        DecoderThreadParts { commands: self.commands, ready: self.ready, lifecycle: self.lifecycle }
    }
}
