use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::{mpsc, watch};

use crate::{
    media::frame::Frame,
    services::codec_service::{
        CodecWorkerError,
        worker::{CodecOutput, WorkerCompletion},
    },
};

pub struct DecoderOutput {
    output: CodecOutput<Arc<Frame>>,
}

impl DecoderOutput {
    pub(in crate::services::codec_service) fn new(
        receiver: mpsc::Receiver<Arc<Frame>>,
        completion: watch::Receiver<WorkerCompletion>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { output: CodecOutput::new(receiver, completion, publishing) }
    }

    pub async fn recv(&mut self) -> Option<Result<Arc<Frame>, CodecWorkerError>> {
        self.output.recv().await
    }
}
