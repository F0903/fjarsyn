//! Serialized session event loop and its protocol-specific responsibilities.

mod application_data;
mod commands;
#[path = "runtime.rs"]
mod implementation;
mod lifecycle;
mod readiness;
mod restart;
mod rtc_events;
mod signaling;

#[cfg(test)]
mod tests;

use implementation::Runtime;
pub(in crate::peer_session) use implementation::spawn;
