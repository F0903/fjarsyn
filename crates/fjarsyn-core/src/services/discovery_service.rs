use std::fmt;

use tokio::sync::mpsc;

use crate::networking::discovery::{Discovery, DiscoveryEvent};

#[derive(thiserror::Error, Debug)]
pub enum DiscoveryServiceError {
    #[error("Failed to start mDNS discovery daemon: {0}")]
    Create(#[source] mdns_sd::Error),
    #[error("Failed to advertise local peer: {0}")]
    Advertise(#[source] mdns_sd::Error),
    #[error("Failed to browse for peers: {0}")]
    Browse(#[source] mdns_sd::Error),
}

pub struct DiscoveryService {
    _discovery: Discovery,
    local_peer_id: String,
    signaling_port: u16,
}

impl DiscoveryService {
    pub fn init(
        local_peer_id: String,
        signaling_port: u16,
        event_tx: mpsc::Sender<DiscoveryEvent>,
    ) -> Result<Self, DiscoveryServiceError> {
        let discovery = Discovery::new().map_err(DiscoveryServiceError::Create)?;
        discovery
            .advertise(&local_peer_id, signaling_port)
            .map_err(DiscoveryServiceError::Advertise)?;
        discovery.browse(event_tx).map_err(DiscoveryServiceError::Browse)?;

        Ok(Self { _discovery: discovery, local_peer_id, signaling_port })
    }
}

impl fmt::Debug for DiscoveryService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscoveryService")
            .field("local_peer_id", &self.local_peer_id)
            .field("signaling_port", &self.signaling_port)
            .finish()
    }
}
