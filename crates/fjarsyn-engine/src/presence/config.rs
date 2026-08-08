use std::time::Duration;

use super::Error;
use crate::identity::PeerId;

/// Hard application-level bounds for unauthenticated mDNS presence state.
///
/// Presence admission is deliberately stable under pressure: an observation
/// for an already-admitted instance may refresh or replace that instance, but
/// a new peer or instance that would exceed a limit is ignored. Existing state
/// is never evicted merely because an unauthenticated newcomer arrived, and a
/// later removal frees capacity for a subsequent observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum distinct peer IDs retained at once. Defaults to 256.
    pub max_peers: usize,
    /// Maximum current mDNS instances retained for one claimed peer ID.
    /// Defaults to 4.
    pub max_advertisements_per_peer: usize,
    /// Maximum de-duplicated endpoints retained from one advertisement.
    /// Defaults to 16.
    pub max_endpoints_per_advertisement: usize,
    /// Maximum de-duplicated endpoints exposed for one aggregate nearby peer.
    /// Defaults to 32.
    pub max_endpoints_per_peer: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_peers: 256,
            max_advertisements_per_peer: 4,
            max_endpoints_per_advertisement: 16,
            max_endpoints_per_peer: 32,
        }
    }
}

impl Limits {
    pub(super) fn zero_limit(self) -> Option<&'static str> {
        [
            ("max_peers", self.max_peers),
            ("max_advertisements_per_peer", self.max_advertisements_per_peer),
            ("max_endpoints_per_advertisement", self.max_endpoints_per_advertisement),
            ("max_endpoints_per_peer", self.max_endpoints_per_peer),
        ]
        .into_iter()
        .find_map(|(name, value)| (value == 0).then_some(name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub peer_id: PeerId,
    pub signaling_port: u16,
    pub instance_name: Option<String>,
    pub hostname: Option<String>,
    pub limits: Limits,
    pub shutdown_timeout: Duration,
}

impl Config {
    pub fn new(peer_id: PeerId, signaling_port: u16) -> Self {
        Self {
            peer_id,
            signaling_port,
            instance_name: None,
            hostname: None,
            limits: Limits::default(),
            shutdown_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_instance_name(mut self, instance_name: impl Into<String>) -> Self {
        self.instance_name = Some(instance_name.into());
        self
    }

    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.shutdown_timeout = shutdown_timeout;
        self
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.signaling_port == 0 {
            return Err(Error::InvalidSignalingPort);
        }
        if self.instance_name.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(Error::InvalidInstanceName);
        }
        if self.hostname.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(Error::InvalidHostname);
        }
        if let Some(name) = self.limits.zero_limit() {
            return Err(Error::InvalidLimit { name });
        }
        if self.shutdown_timeout.is_zero() {
            return Err(Error::InvalidShutdownTimeout);
        }
        Ok(())
    }
}
