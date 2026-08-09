//! Remote receive, decode, and authenticated-share lifecycle ownership.

mod controller;
mod h264;
mod pipeline;

pub(in crate::screen_share) use controller::Controller;
use pipeline::Pipeline;
