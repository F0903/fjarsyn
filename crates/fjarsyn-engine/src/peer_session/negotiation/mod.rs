//! Authenticated signaling transport and peer-session negotiation.

use std::time::Duration;

use chrono::Duration as ChronoDuration;

mod admission;
mod connection;
mod incoming;
mod listener;
mod service;
mod tls;

#[derive(Debug, Clone)]
pub(in crate::peer_session) struct Limits {
    pub max_frame_bytes: usize,
    pub queue_capacity: usize,
    pub replay_capacity: usize,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub authentication_global_burst: usize,
    pub authentication_global_refill_interval: Duration,
    pub authentication_per_ip_burst: usize,
    pub authentication_per_ip_refill_interval: Duration,
    pub max_authentication_tracked_ips: usize,
    pub max_endpoint_attempts: usize,
    pub endpoint_attempt_timeout: Duration,
    pub handshake_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_message_age: ChronoDuration,
    pub max_clock_skew: ChronoDuration,
}

pub(in crate::peer_session) use connection::Connection;
use connection::SessionConnectionContext;
#[cfg(test)]
use connection::{
    receive_handshake_envelope, secure_websocket_url, send_handshake_envelope, websocket_config,
};
pub(in crate::peer_session) use incoming::{Incoming, Intent};
pub(in crate::peer_session) use listener::Listener;
pub(in crate::peer_session) use service::Service;

#[cfg(test)]
fn test_limits() -> Limits {
    Limits {
        max_frame_bytes: 16 * 1024,
        queue_capacity: 8,
        replay_capacity: 8,
        max_connections: 4,
        max_connections_per_ip: 4,
        authentication_global_burst: 32,
        authentication_global_refill_interval: Duration::from_millis(100),
        authentication_per_ip_burst: 8,
        authentication_per_ip_refill_interval: Duration::from_millis(500),
        max_authentication_tracked_ips: 32,
        max_endpoint_attempts: 6,
        endpoint_attempt_timeout: Duration::from_secs(1),
        handshake_timeout: Duration::from_secs(1),
        idle_timeout: Duration::from_secs(5),
        max_message_age: ChronoDuration::minutes(5),
        max_clock_skew: ChronoDuration::seconds(30),
    }
}

#[cfg(test)]
mod tests;
