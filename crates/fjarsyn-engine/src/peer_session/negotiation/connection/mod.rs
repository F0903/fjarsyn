//! Authenticated signaling connections and their lifetime contracts.

mod endpoint_plan;
mod handshake;
#[path = "connection.rs"]
mod implementation;
mod socket_runtime;

#[cfg(test)]
pub(in crate::peer_session::negotiation) use endpoint_plan::secure_websocket_url;
pub(in crate::peer_session::negotiation) use handshake::{
    receive_handshake_envelope, send_handshake_envelope, websocket_config,
};
pub(in crate::peer_session) use implementation::Connection;
pub(in crate::peer_session::negotiation) use implementation::{
    ConnectionPermits, IpConnectionPermit, SessionConnectionContext,
};
