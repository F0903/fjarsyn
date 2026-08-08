//! Wgpu pipeline and primitive backing the GPU frame viewer.

#[path = "gpu_frame_viewer.rs"]
mod implementation;
mod pipeline;

pub(in crate::ui) use implementation::GpuFrameViewer;
use pipeline::Pipeline;
