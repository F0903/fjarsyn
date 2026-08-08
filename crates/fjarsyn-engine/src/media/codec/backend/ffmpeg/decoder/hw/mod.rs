//! Hardware-decoder selection and platform-specific decode backends.

mod backend;
#[cfg(target_os = "windows")]
mod d3d11va;

pub(super) use backend::Backend;
