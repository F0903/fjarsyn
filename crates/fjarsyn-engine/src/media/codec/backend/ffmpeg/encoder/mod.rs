//! Low-level FFmpeg video encoding, scaling, and platform acceleration.

#[path = "encoder.rs"]
mod implementation;
mod software;

#[cfg(target_os = "windows")]
mod windows;

pub(crate) use implementation::Encoder;
use implementation::{Error, Result};
