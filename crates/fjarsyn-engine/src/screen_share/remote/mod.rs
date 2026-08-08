//! Remote receive, decode, and authenticated-share lifecycle ownership.

mod controller;
mod h264;
mod pipeline;

pub(in crate::screen_share) use controller::Controller;
use h264::contains_nal_type;
use pipeline::Pipeline;

use crate::screen_share::ShareBinding;
