use serde::{Deserialize, Serialize};

use crate::peer_session::Error;

/// Monotonic epoch for one peer's screen shares within a session.
///
/// `ShareId` is the application identity presented to callers. `ShareEpoch` is
/// the compact, ordered media-plane boundary carried on RTP packets so a
/// receiver can distinguish delayed old media from early media for the next
/// share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShareEpoch(u64);

impl ShareEpoch {
    pub const FIRST: Self = Self(1);

    pub const fn value(self) -> u64 {
        self.0
    }

    pub(in crate::peer_session) const fn from_value(value: u64) -> Self {
        Self(value)
    }

    pub(in crate::peer_session) fn next(self) -> Result<Self, Error> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| Error::Protocol("screen-share epoch overflowed".into()))
    }

    pub(in crate::peer_session) fn require_valid(self) -> Result<(), Error> {
        if self.0 == 0 {
            Err(Error::Protocol("screen-share epoch must be non-zero".into()))
        } else {
            Ok(())
        }
    }
}

impl TryFrom<u64> for ShareEpoch {
    type Error = Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let epoch = Self(value);
        epoch.require_valid()?;
        Ok(epoch)
    }
}
