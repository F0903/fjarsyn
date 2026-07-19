use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::mpsc;

use crate::media::frame::Frame;

#[derive(Clone)]
pub struct EncoderInput {
    sender: mpsc::Sender<Arc<Frame>>,
    accepting: Arc<AtomicBool>,
}

impl EncoderInput {
    pub(in crate::services::codec_service) fn new(
        sender: mpsc::Sender<Arc<Frame>>,
        accepting: Arc<AtomicBool>,
    ) -> Self {
        Self { sender, accepting }
    }

    pub fn try_send(&self, frame: Arc<Frame>) -> Result<(), mpsc::error::TrySendError<Arc<Frame>>> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(mpsc::error::TrySendError::Closed(frame));
        }
        self.sender.try_send(frame)
    }
}
