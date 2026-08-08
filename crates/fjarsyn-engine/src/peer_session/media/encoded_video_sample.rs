use std::time::Duration;

use bytes::Bytes;

/// One encoded H.264 access unit and its intended media duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedVideoSample {
    pub(crate) data: Bytes,
    pub(crate) duration: Duration,
}

impl EncodedVideoSample {
    pub(crate) fn new(data: impl Into<Bytes>, duration: Duration) -> Self {
        Self { data: data.into(), duration }
    }
}
