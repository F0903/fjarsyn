//! Application-facing decoder session and its input/output channel halves.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use tokio::sync::{mpsc, watch};

use crate::media::{
    codec::{
        Worker, WorkerError,
        worker::{WorkerCompletion, WorkerOutput},
    },
    frame::Frame,
};

#[derive(Clone)]
pub struct DecoderInput {
    sender: mpsc::Sender<Bytes>,
    accepting: Arc<AtomicBool>,
}

impl DecoderInput {
    pub(in crate::media::codec) fn new(
        sender: mpsc::Sender<Bytes>,
        accepting: Arc<AtomicBool>,
    ) -> Self {
        Self { sender, accepting }
    }

    pub fn try_send(&self, packet: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(mpsc::error::TrySendError::Closed(packet));
        }
        self.sender.try_send(packet)
    }
}

pub struct DecoderOutput {
    output: WorkerOutput<Arc<Frame>>,
}

impl DecoderOutput {
    pub(in crate::media::codec) fn new(
        receiver: mpsc::Receiver<Arc<Frame>>,
        completion: watch::Receiver<WorkerCompletion>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { output: WorkerOutput::new(receiver, completion, publishing) }
    }

    pub async fn recv(&mut self) -> Option<Result<Arc<Frame>, WorkerError>> {
        self.output.recv().await
    }
}

pub struct DecoderSessionParts {
    pub input: DecoderInput,
    pub output: DecoderOutput,
    pub worker: Worker,
}

pub struct DecoderSession {
    input: DecoderInput,
    output: DecoderOutput,
    worker: Worker,
}

impl DecoderSession {
    pub(in crate::media::codec) fn new(
        input: DecoderInput,
        output: DecoderOutput,
        worker: Worker,
    ) -> Self {
        Self { input, output, worker }
    }

    pub fn try_send(&self, packet: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        self.input.try_send(packet)
    }

    pub async fn recv(&mut self) -> Option<Result<Arc<Frame>, WorkerError>> {
        self.output.recv().await
    }

    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn request_stop(&self) {
        self.worker.request_stop();
    }

    pub async fn shutdown(self) -> Result<(), WorkerError> {
        self.worker.shutdown().await
    }

    pub fn into_parts(self) -> DecoderSessionParts {
        DecoderSessionParts { input: self.input, output: self.output, worker: self.worker }
    }
}
