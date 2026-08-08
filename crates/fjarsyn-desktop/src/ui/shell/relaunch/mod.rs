//! Orderly application shutdown and replacement-process launch.

mod process;
mod reducer;
mod workflow;

pub(super) use process::relaunch_current_executable;
pub(super) use reducer::{begin, finish};
pub(super) use workflow::shutdown_then_launch;
