use std::sync::Arc;

use fjarsyn_core::media::frame::Frame;
use tokio::sync::{Mutex, watch};

#[derive(Clone)]
pub(crate) struct LatestFrameReceiverRef(pub Arc<Mutex<watch::Receiver<Option<Arc<Frame>>>>>);

impl LatestFrameReceiverRef {
    pub(super) fn new(receiver: watch::Receiver<Option<Arc<Frame>>>) -> Self {
        Self(Arc::new(Mutex::new(receiver)))
    }
}

impl std::fmt::Debug for LatestFrameReceiverRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LatestFrameReceiverRef")
    }
}

impl std::hash::Hash for LatestFrameReceiverRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl PartialEq for LatestFrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LatestFrameReceiverRef {}
