//! Application shell, state model, update routing, and top-level presentation.

mod application;
mod handlers;
mod relaunch;
mod state;
mod view;
mod window_workflow;

pub(in crate::ui) use application::Fjarsyn;
pub(in crate::ui::shell) use state::{Lifecycle, Runtime, State, UiState, WindowInfo};
