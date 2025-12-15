use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

// A wrapper around an Arc of Mutex<mpsc::Receiver<Bytes>>
#[derive(Clone)]
pub struct FrameReceiverRef(pub Arc<Mutex<mpsc::Receiver<Bytes>>>);

impl Hash for FrameReceiverRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl PartialEq for FrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FrameReceiverRef {}
