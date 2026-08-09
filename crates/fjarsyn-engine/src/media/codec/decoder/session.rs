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

    /// Waits for bounded decoder-input capacity instead of discarding an
    /// arbitrary encoded access unit when the worker is busy.
    pub async fn send(&self, packet: Bytes) -> Result<(), mpsc::error::SendError<Bytes>> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(mpsc::error::SendError(packet));
        }
        let permit = match self.sender.reserve().await {
            Ok(permit) => permit,
            Err(_) => return Err(mpsc::error::SendError(packet)),
        };
        if !self.accepting.load(Ordering::Acquire) {
            return Err(mpsc::error::SendError(packet));
        }
        permit.send(packet);
        Ok(())
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

    pub async fn send(&self, packet: Bytes) -> Result<(), mpsc::error::SendError<Bytes>> {
        self.input.send(packet).await
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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use bytes::Bytes;
    use tokio::sync::mpsc;

    use super::DecoderInput;

    #[tokio::test]
    async fn awaited_send_applies_capacity_backpressure() {
        let (sender, mut receiver) = mpsc::channel(1);
        let input = DecoderInput::new(sender, Arc::new(AtomicBool::new(true)));
        input.send(Bytes::from_static(b"first")).await.unwrap();

        let blocked = tokio::spawn({
            let input = input.clone();
            async move { input.send(Bytes::from_static(b"second")).await }
        });
        tokio::task::yield_now().await;
        assert!(!blocked.is_finished());

        assert_eq!(receiver.recv().await.unwrap(), Bytes::from_static(b"first"));
        blocked.await.unwrap().unwrap();
        assert_eq!(receiver.recv().await.unwrap(), Bytes::from_static(b"second"));
    }

    #[tokio::test]
    async fn awaited_send_rejects_a_stopped_worker() {
        let accepting = Arc::new(AtomicBool::new(true));
        let (sender, _receiver) = mpsc::channel(1);
        let input = DecoderInput::new(sender, accepting.clone());
        accepting.store(false, Ordering::Release);

        let packet = Bytes::from_static(b"stale");
        let error = input.send(packet.clone()).await.unwrap_err();
        assert_eq!(error.0, packet);
    }
}
