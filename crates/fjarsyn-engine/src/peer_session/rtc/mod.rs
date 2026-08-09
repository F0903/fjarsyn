//! WebRTC transport, channels, media, and event adaptation.

use std::time::Duration;

use crate::peer_session::NetworkScope;

mod channel_kind;
mod event;
mod peer;
mod share_epoch;

#[derive(Debug, Clone)]
pub(in crate::peer_session) struct Config {
    pub network_scope: NetworkScope,
    pub ice_servers: Vec<String>,
    pub max_depacket_latency: Duration,
    pub max_candidates_per_generation: usize,
    pub max_data_message_bytes: usize,
    pub operation_timeout: Duration,
}

pub(in crate::peer_session) use channel_kind::ChannelKind;
pub(in crate::peer_session) use event::Event;
use event::EventDispatcher;
pub(in crate::peer_session) use peer::Peer;
