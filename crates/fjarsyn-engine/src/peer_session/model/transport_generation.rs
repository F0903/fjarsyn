use serde::{Deserialize, Serialize};

use crate::peer_session::Error;

/// Monotonically identifies the ICE credentials and callbacks belonging to one
/// negotiated transport. Generation zero is the initial connection; a restart
/// may only propose the exact next generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::peer_session) struct TransportGeneration(u64);

impl TransportGeneration {
    pub(in crate::peer_session) const INITIAL: Self = Self(0);

    pub(in crate::peer_session) const fn from_value(value: u64) -> Self {
        Self(value)
    }

    pub(in crate::peer_session) const fn value(self) -> u64 {
        self.0
    }

    pub(in crate::peer_session) fn next(self) -> Result<Self, Error> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| Error::Protocol("transport generation exhausted".into()))
    }
}
