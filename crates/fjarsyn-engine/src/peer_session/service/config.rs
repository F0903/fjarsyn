use std::{fmt, sync::Arc, time::Duration};

use tokio::sync::mpsc;

use super::{
    endpoint_resolver::EndpointResolver, limits::Limits, trusted_peer_resolver::TrustedPeerResolver,
};
use crate::{
    identity::{PeerId, StoredIdentityKeypair},
    peer_session::Event,
};

#[derive(Clone)]
pub struct Config {
    pub local_peer_id: Option<PeerId>,
    pub identity_keypair: Option<StoredIdentityKeypair>,
    pub trusted_peers: Arc<dyn TrustedPeerResolver>,
    pub endpoints: Arc<dyn EndpointResolver>,
    pub signaling_port: u16,
    pub ice_servers: Vec<String>,
    pub max_depacket_latency: Duration,
    pub limits: Limits,
    /// Mandatory, ordered persistence/event consumer. If this bounded queue
    /// closes or fills, all sessions are failed rather than dropping events.
    pub mandatory_event_sink: Option<mpsc::Sender<Event>>,
}

impl Config {
    pub fn new(
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        endpoints: Arc<dyn EndpointResolver>,
    ) -> Self {
        Self {
            local_peer_id: None,
            identity_keypair: None,
            trusted_peers,
            endpoints,
            signaling_port: 0,
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            limits: Limits::default(),
            mandatory_event_sink: None,
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("local_peer_id", &self.local_peer_id)
            .field("has_identity_keypair", &self.identity_keypair.is_some())
            .field("signaling_port", &self.signaling_port)
            .field("ice_servers", &self.ice_servers)
            .field("max_depacket_latency", &self.max_depacket_latency)
            .field("limits", &self.limits)
            .field("has_mandatory_event_sink", &self.mandatory_event_sink.is_some())
            .finish_non_exhaustive()
    }
}
