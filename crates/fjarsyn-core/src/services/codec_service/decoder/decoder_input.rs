use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bytes::Bytes;
use tokio::sync::mpsc;

pub struct DecoderInput {
    sender: mpsc::Sender<Bytes>,
    accepting: Arc<AtomicBool>,
}

impl DecoderInput {
    pub(in crate::services::codec_service) fn new(
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

impl Clone for DecoderInput {
    fn clone(&self) -> Self {
        Self { sender: self.sender.clone(), accepting: self.accepting.clone() }
    }
}
