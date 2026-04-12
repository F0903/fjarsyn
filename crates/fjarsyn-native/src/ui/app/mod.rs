mod bootstrap;
mod global;
mod state;
mod tasks;
mod update;
mod view;
mod workflows;

pub mod handlers;

pub use state::{
    APP_TITLE, AppContext, AppContextMut, AppRuntime, AppState, ContactsState, Fjarsyn, MediaState,
    MessagingState, NetworkingState, RuntimeServices, ServicesState, SessionState, ShellState,
    UIState, WindowInfo,
};

pub(crate) use crate::ui::screens::{ActiveScreen, Screen};
