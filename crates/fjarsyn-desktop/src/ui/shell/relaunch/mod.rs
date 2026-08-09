//! Orderly engine-runtime shutdown and replacement-process launch.

mod process;
mod reducer;

pub(super) use process::relaunch_current_executable;
pub(super) use reducer::{AfterShutdown, begin, finish, shutdown_finished};
