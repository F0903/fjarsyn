use std::{fmt, sync::Arc, time::Duration};

use tokio::sync::mpsc;

use super::{
    endpoint_resolver::EndpointResolver, limits::Limits, trusted_peer_resolver::TrustedPeerResolver,
};
use crate::{
    identity::LocalIdentity,
    peer_session::{Event, NetworkScope},
};

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) local_identity: LocalIdentity,
    pub(crate) trusted_peers: Arc<dyn TrustedPeerResolver>,
    pub(crate) endpoints: Arc<dyn EndpointResolver>,
    pub(crate) network_scope: NetworkScope,
    pub(crate) signaling_port: u16,
    pub(crate) ice_servers: Vec<String>,
    pub(crate) max_depacket_latency: Duration,
    pub(crate) limits: Limits,
    /// Mandatory, ordered persistence/event consumer. If this bounded queue
    /// closes or fills, all sessions are failed rather than dropping events.
    pub(crate) mandatory_event_sink: Option<mpsc::Sender<Event>>,
}

impl Config {
    pub(crate) fn new(
        local_identity: LocalIdentity,
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        endpoints: Arc<dyn EndpointResolver>,
        network_scope: NetworkScope,
    ) -> Self {
        Self {
            local_identity,
            trusted_peers,
            endpoints,
            network_scope,
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
            .field("local_peer_id", self.local_identity.peer_id())
            .field("network_scope", &self.network_scope)
            .field("signaling_port", &self.signaling_port)
            .field("ice_servers", &self.ice_servers)
            .field("max_depacket_latency", &self.max_depacket_latency)
            .field("limits", &self.limits)
            .field("has_mandatory_event_sink", &self.mandatory_event_sink.is_some())
            .finish_non_exhaustive()
    }
}
