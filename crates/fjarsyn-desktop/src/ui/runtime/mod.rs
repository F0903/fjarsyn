//! Desktop ownership around the headless engine and its UI-facing adapter.

mod engine_adapter;
mod engine_runtime;
mod retained;
mod runtime_id;

pub(in crate::ui) use engine_adapter::{
    EngineState, Failure as EngineAdapterFailure, Notice as EngineNotice,
    Receivers as EngineReceivers,
};
pub(in crate::ui) use engine_runtime::{EngineRuntime, RuntimeSlot};
pub(in crate::ui) use retained::Retained;
pub(in crate::ui) use runtime_id::RuntimeId;
