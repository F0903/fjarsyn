use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::mpsc;

use super::{decoder_input::DecoderInput, decoder_output::DecoderOutput};
use crate::{
    media::frame::Frame,
    services::codec_service::{CodecWorker, CodecWorkerError},
};

pub struct DecoderSession {
    input: DecoderInput,
    output: DecoderOutput,
    worker: CodecWorker,
}

pub struct DecoderSessionParts {
    pub input: DecoderInput,
    pub output: DecoderOutput,
    pub worker: CodecWorker,
}

impl DecoderSession {
    pub(in crate::services::codec_service) fn new(
        input: DecoderInput,
        output: DecoderOutput,
        worker: CodecWorker,
    ) -> Self {
        Self { input, output, worker }
    }

    pub fn try_send(&self, packet: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        self.input.try_send(packet)
    }

    pub async fn recv(&mut self) -> Option<Result<Arc<Frame>, CodecWorkerError>> {
        self.output.recv().await
    }

    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn request_stop(&self) {
        self.worker.request_stop();
    }

    pub async fn shutdown(self) -> Result<(), CodecWorkerError> {
        self.worker.shutdown().await
    }

    pub fn into_parts(self) -> DecoderSessionParts {
        DecoderSessionParts { input: self.input, output: self.output, worker: self.worker }
    }
}
