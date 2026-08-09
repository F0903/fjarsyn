//! Shared codec test fixtures and scripted backends.

mod blocking_gate;
mod fixtures;
mod scripted_backend_factory;

pub(super) use blocking_gate::{BlockingGate, ReleaseGateOnDrop};
pub(super) use fixtures::{
    decoder_config, encoder_config, test_frame, test_frame_without_duration, test_service,
    wait_until_reaped,
};
pub(super) use scripted_backend_factory::{DecoderPlan, EncoderPlan, ScriptedBackendFactory};
