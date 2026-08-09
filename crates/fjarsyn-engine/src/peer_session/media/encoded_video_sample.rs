use std::time::Duration;

use bytes::Bytes;

/// One encoded H.264 access unit, its intended media duration, and whether the
/// receiver observed loss immediately before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedVideoSample {
    pub(crate) data: Bytes,
    pub(crate) duration: Duration,
    starts_after_discontinuity: bool,
}

impl EncodedVideoSample {
    pub(crate) fn new(data: impl Into<Bytes>, duration: Duration) -> Self {
        Self { data: data.into(), duration, starts_after_discontinuity: false }
    }

    pub(in crate::peer_session) fn received(
        data: impl Into<Bytes>,
        duration: Duration,
        starts_after_discontinuity: bool,
    ) -> Self {
        Self { data: data.into(), duration, starts_after_discontinuity }
    }

    pub(crate) const fn starts_after_discontinuity(&self) -> bool {
        self.starts_after_discontinuity
    }

    pub(in crate::peer_session) fn mark_discontinuous(&mut self) {
        self.starts_after_discontinuity = true;
    }
}
