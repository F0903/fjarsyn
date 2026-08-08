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
