//! Dedicated codec worker execution, lifecycle state, and completion reporting.

#[path = "worker.rs"]
mod implementation;
mod supervisor_tasks;
mod worker_apartment;
mod worker_lifecycle;
mod worker_output;

pub(in crate::media::codec) use implementation::WorkerCompletion;
pub use implementation::{Worker, WorkerError};
pub(in crate::media::codec) use supervisor_tasks::SupervisorTasks;
pub(in crate::media::codec) use worker_apartment::WorkerApartment;
pub(in crate::media::codec) use worker_lifecycle::WorkerLifecycle;
pub(in crate::media::codec) use worker_output::WorkerOutput;
