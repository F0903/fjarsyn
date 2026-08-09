//! Application-facing encoder session and its input/output channel halves.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::{mpsc, watch};

use crate::media::{
    codec::{
        Worker, WorkerError,
        worker::{WorkerCompletion, WorkerOutput},
    },
    frame::Frame,
};

#[derive(Clone)]
pub struct EncoderInput {
    sender: mpsc::Sender<Arc<Frame>>,
    accepting: Arc<AtomicBool>,
    keyframe_requested: Arc<AtomicBool>,
}

impl EncoderInput {
    pub(in crate::media::codec) fn new(
        sender: mpsc::Sender<Arc<Frame>>,
        accepting: Arc<AtomicBool>,
        keyframe_requested: Arc<AtomicBool>,
    ) -> Self {
        Self { sender, accepting, keyframe_requested }
    }

    pub fn try_send(&self, frame: Arc<Frame>) -> Result<(), mpsc::error::TrySendError<Arc<Frame>>> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(mpsc::error::TrySendError::Closed(frame));
        }
        self.sender.try_send(frame)
    }

    /// Coalesces a request to encode the next encodable frame as a keyframe.
    pub fn request_keyframe(&self) {
        self.keyframe_requested.store(true, Ordering::Release);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub nal_units: Vec<Vec<u8>>,
    pub duration: Duration,
}

pub struct EncoderOutput {
    output: WorkerOutput<EncodedFrame>,
}

impl EncoderOutput {
    pub(in crate::media::codec) fn new(
        receiver: mpsc::Receiver<EncodedFrame>,
        completion: watch::Receiver<WorkerCompletion>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { output: WorkerOutput::new(receiver, completion, publishing) }
    }

    pub async fn recv(&mut self) -> Option<Result<EncodedFrame, WorkerError>> {
        self.output.recv().await
    }
}

pub struct EncoderSessionParts {
    pub input: EncoderInput,
    pub output: EncoderOutput,
    pub worker: Worker,
}

pub struct EncoderSession {
    input: EncoderInput,
    output: EncoderOutput,
    worker: Worker,
}

impl EncoderSession {
    pub(in crate::media::codec) fn new(
        input: EncoderInput,
        output: EncoderOutput,
        worker: Worker,
    ) -> Self {
        Self { input, output, worker }
    }

    pub fn try_send(&self, frame: Arc<Frame>) -> Result<(), mpsc::error::TrySendError<Arc<Frame>>> {
        self.input.try_send(frame)
    }

    /// Coalesces a request to encode the next encodable frame as a keyframe.
    pub fn request_keyframe(&self) {
        self.input.request_keyframe();
    }

    pub async fn recv(&mut self) -> Option<Result<EncodedFrame, WorkerError>> {
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

    pub fn into_parts(self) -> EncoderSessionParts {
        EncoderSessionParts { input: self.input, output: self.output, worker: self.worker }
    }
}
