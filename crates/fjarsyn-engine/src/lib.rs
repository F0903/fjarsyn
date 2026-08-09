//! Fjarsyn's headless application engine.
//!
//! [`Engine`] is the canonical owner of database persistence and capability
//! lifecycles. The crate exposes trust-safe commands, immutable
//! projections, capture, and media capabilities without depending on a
//! user-interface framework. Concrete hosted-service implementations and their
//! startup configuration remain private; callers interact through [`Services`]
//! and capability-specific handles.

#![deny(unreachable_pub)]

pub mod contacts;
mod database;
mod deferred_resolver;
mod engine;
mod error;
pub mod identity;
pub mod media;
pub mod messaging;
pub mod pairing;
mod paths;
pub mod peer_session;
pub mod presence;
pub mod screen_share;
pub mod service_host;
mod services;
pub mod settings;

pub use engine::Engine;
pub use error::{ShutdownError, StartError, StartupStage};
pub use services::Services;
