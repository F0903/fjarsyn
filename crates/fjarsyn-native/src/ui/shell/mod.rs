mod bootstrap;
mod global;
mod state;
mod tasks;
mod update;
mod view;
mod workflows;

pub mod handlers;

pub use state::{
    APP_TITLE, AppLifecycle, AppState, ContactsState, Fjarsyn, MediaState, MessagingState,
    NetworkingState, Services, ServicesState, SessionState, ShellContext, ShellContextMut,
    ShellRuntime, ShellState, UIState, WindowInfo,
};

pub(crate) use crate::ui::screens::{ActiveScreen, Screen};
