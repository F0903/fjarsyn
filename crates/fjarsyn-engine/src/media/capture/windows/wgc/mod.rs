//! Windows Graphics Capture provider construction and runtime responsibilities.

mod builder;
mod provider;
mod runtime;

pub use builder::{Builder, BuilderError};
pub use provider::{Provider, Stream};
