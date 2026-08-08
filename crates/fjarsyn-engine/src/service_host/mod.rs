//! Generic ownership and lifecycle infrastructure for hosted services.
//!
//! A [`HostedService`] owns independent execution and exposes a typed handle.
//! [`ServiceHost`] retains the implementation and coordinates preparation,
//! graceful shutdown, failure attribution, and cancellation without knowing
//! anything about Fjarsyn's concrete capabilities.

mod hosted_service;
#[path = "service_host.rs"]
mod implementation;
mod managed_service;
mod service_failure;
mod shutdown_context;

#[cfg(test)]
mod tests;

pub use hosted_service::HostedService;
pub use implementation::{ServiceHost, ServicePolicy};
pub use service_failure::ServiceFailure;
pub use shutdown_context::ShutdownContext;
