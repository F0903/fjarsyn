//! Service event-loop wiring and its owned session lifecycle responsibilities.

mod commands;
#[path = "runtime.rs"]
mod implementation;
mod incoming;
mod recent_session_ids;
mod sessions;
mod shutdown;

pub(in crate::peer_session::service) use implementation::Runtime;
use implementation::SessionEntry;
use recent_session_ids::RecentSessionIds;
