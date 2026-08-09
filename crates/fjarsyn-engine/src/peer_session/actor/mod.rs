//! Owns one live peer session and serializes its state transitions.

mod application_data_gate;
mod command;
mod config;
mod handle;
mod report;
pub(in crate::peer_session) mod restart;
mod runtime;
mod state_machine;
mod task_supervision;

use application_data_gate::ApplicationDataGate;
pub(in crate::peer_session) use command::Command;
pub(in crate::peer_session) use config::{Config, Role};
use handle::Control;
pub(in crate::peer_session) use handle::Handle;
pub(in crate::peer_session) use report::{TaskExit, Terminal, Update};
pub(in crate::peer_session) use runtime::spawn;

/// Opaque identity of one concrete peer-session actor instance.
///
/// A session ID may be reused after an actor exits. This identity lets the
/// orchestrator reject delayed reports from the actor that previously owned
/// that session ID without implying any ordering between actor instances.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::peer_session) struct ActorInstanceId(uuid::Uuid);

impl ActorInstanceId {
    pub(in crate::peer_session) fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
