use crate::{
    identity::PeerId,
    peer_session::{SessionId, negotiation, rtc},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::peer_session) enum Role {
    Outgoing,
    Incoming,
}

pub(in crate::peer_session) struct Config {
    pub session_id: SessionId,
    pub remote_peer_id: PeerId,
    pub remote_public_key: String,
    pub role: Role,
    pub connection: Option<negotiation::Connection>,
    pub negotiation: negotiation::Service,
    pub rtc: rtc::Config,
    pub command_capacity: usize,
    pub media_capacity: usize,
    pub remote_video_capacity: usize,
    pub max_message_bytes: usize,
    pub max_data_message_bytes: usize,
    pub request_timeout: std::time::Duration,
    pub negotiation_timeout: std::time::Duration,
    pub event_delivery_timeout: std::time::Duration,
    pub cleanup_timeout: std::time::Duration,
    pub pre_ready_data_capacity: usize,
    pub disconnected_grace: std::time::Duration,
    pub ice_restart_timeout: std::time::Duration,
    pub max_remote_timestamp_age: std::time::Duration,
    pub max_remote_clock_skew: std::time::Duration,
}
