//! Local capture, encode, and authenticated-share lifecycle ownership.

mod capture;
mod controller;
mod plan;

use capture::{CaptureGuard, requires_capture_readback, stop_capture};
pub(in crate::screen_share) use controller::Controller;
pub(in crate::screen_share) use plan::Plan;
