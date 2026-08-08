//! Reusable Iced widgets and application chrome.

mod frame_viewer;
mod notifications;
mod sidebar;
mod window_chrome;

pub(in crate::ui) use frame_viewer::{CpuFrameViewer, GpuFrameViewer};
pub(in crate::ui) use notifications::notifications_view;
pub(in crate::ui) use sidebar::sidebar;
pub(in crate::ui) use window_chrome::{resize_grid, titlebar, window_controls};
