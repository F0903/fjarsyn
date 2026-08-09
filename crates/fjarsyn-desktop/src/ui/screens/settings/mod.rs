//! Settings state, validation workflow, and tabbed presentation.

mod components;
mod screen;
mod settings_draft;
mod tabs;
mod view;
mod workflow;

pub(super) use screen::Screen;
use settings_draft::SettingsDraft;
use tabs::Tab;
