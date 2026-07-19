use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::{mpsc, watch};

use crate::services::codec_service::{
    CodecWorkerError, EncodedFrame,
    worker::{CodecOutput, WorkerCompletion},
};

pub struct EncoderOutput {
    output: CodecOutput<EncodedFrame>,
}

impl EncoderOutput {
    pub(in crate::services::codec_service) fn new(
        receiver: mpsc::Receiver<EncodedFrame>,
        completion: watch::Receiver<WorkerCompletion>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { output: CodecOutput::new(receiver, completion, publishing) }
    }

    pub async fn recv(&mut self) -> Option<Result<EncodedFrame, CodecWorkerError>> {
        self.output.recv().await
    }
}
