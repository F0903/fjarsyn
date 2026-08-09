//! Wgpu pipeline and primitive backing the GPU frame viewer.

mod frame_cache;
#[path = "gpu_frame_viewer.rs"]
mod implementation;
mod pipeline;
mod uniforms;

pub(in crate::ui) use implementation::GpuFrameViewer;
use pipeline::Pipeline;
