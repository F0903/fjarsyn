//! Supervised, bounded adaptation of engine outputs for the desktop runtime.

mod coordinator;
mod engine_state;
mod failure;
#[path = "engine_adapter.rs"]
mod implementation;
mod notice;

pub(in crate::ui) use engine_state::EngineState;
pub(in crate::ui) use failure::Failure;
pub(in crate::ui) use implementation::Receivers;
pub(in crate::ui::runtime) use implementation::{EngineAdapter, Shutdown};
pub(in crate::ui) use notice::Notice;
