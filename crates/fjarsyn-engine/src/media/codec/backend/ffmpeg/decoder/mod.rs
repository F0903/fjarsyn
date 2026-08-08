//! Low-level FFmpeg video decoding and hardware-acceleration integration.

mod hw;
#[path = "decoder.rs"]
mod implementation;

pub(crate) use implementation::Decoder;
use implementation::{Error, Result};
