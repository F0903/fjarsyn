use std::sync::Arc;

use tokio::sync::mpsc;

use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecWorker, CodecWorkerError, EncodedFrame, EncoderInput, EncoderOutput,
    },
};

pub struct EncoderSession {
    input: EncoderInput,
    output: EncoderOutput,
    worker: CodecWorker,
}

pub struct EncoderSessionParts {
    pub input: EncoderInput,
    pub output: EncoderOutput,
    pub worker: CodecWorker,
}

impl EncoderSession {
    pub(in crate::services::codec_service) fn new(
        input: EncoderInput,
        output: EncoderOutput,
        worker: CodecWorker,
    ) -> Self {
        Self { input, output, worker }
    }

    pub fn try_send(&self, frame: Arc<Frame>) -> Result<(), mpsc::error::TrySendError<Arc<Frame>>> {
        self.input.try_send(frame)
    }

    pub async fn recv(&mut self) -> Option<Result<EncodedFrame, CodecWorkerError>> {
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

    pub fn into_parts(self) -> EncoderSessionParts {
        EncoderSessionParts { input: self.input, output: self.output, worker: self.worker }
    }
}
