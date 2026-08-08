//! CPU- and GPU-backed video frame presentation widgets.

mod cpu_frame_viewer;
mod gpu_frame_viewer;

pub(in crate::ui) use cpu_frame_viewer::CpuFrameViewer;
pub(in crate::ui) use gpu_frame_viewer::GpuFrameViewer;
