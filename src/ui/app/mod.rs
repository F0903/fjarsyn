mod bootstrap;
mod global;
mod state;
mod tasks;
mod update;
mod view;
mod workflows;

pub mod handlers;

pub use state::{
    APP_TITLE, AppState, Fjarsyn, MediaState, NetworkingState, Services, SessionState, UIState,
    WindowInfo,
};

pub(crate) use crate::ui::screens::{ActiveScreen, Screen};
