use async_trait::async_trait;

use super::super::{Error, NearbyAdvertisement};
use crate::identity::PeerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::presence) enum Observation {
    Resolved(ResolvedAdvertisement),
    Removed { instance_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::presence) struct ResolvedAdvertisement {
    pub(in crate::presence) peer_id: PeerId,
    pub(in crate::presence) advertisement: NearbyAdvertisement,
}

#[async_trait]
pub(in crate::presence) trait Backend: Send {
    async fn next_observation(&mut self) -> Result<Option<Observation>, Error>;

    async fn shutdown(&mut self) -> Result<(), Error>;
}
