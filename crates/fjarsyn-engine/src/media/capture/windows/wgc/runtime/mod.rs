//! Capture-session execution, frame processing, and recoverable GPU resources.

mod device_loss_recovery;
mod device_state;
mod processing;
mod resource_pool;
mod session;

pub(super) use device_loss_recovery::DeviceLossRecovery;
pub(super) use device_state::DeviceState;
pub(super) use processing::process_frame;
pub(super) use resource_pool::ResourcePool;
pub(super) use session::{SessionSettings, SessionState};

pub(super) const FRAME_BUFFER_COUNT: i32 = 5;
pub(super) const PIPELINE_DEPTH: usize = 3;
