//! Domain-oriented dispatch from aggregate UI messages into shell updates.

use iced::Task;

use crate::ui::{
    message::{self, Message},
    screens::Active,
    shell::Fjarsyn,
};

mod config;
mod contact;
mod lifecycle;
mod peer;
mod runtime;
mod window;

pub(super) use config::handle_config_msg;
pub(super) use contact::handle_contact_operation;
pub(super) use lifecycle::{handle_lifecycle_msg, shutdown};
pub(super) use peer::handle_peer_action;
pub(super) use runtime::handle_runtime_msg;
pub(super) use window::{handle_window_control_msg, handle_window_event_msg};

/// Navigation changes presentation only. It never connects, disconnects, or
/// changes the active media pipeline.
pub(super) fn handle_navigation_msg(
    app: &mut Fjarsyn,
    message: message::Navigation,
) -> Task<Message> {
    let message::Navigation::Navigate(route) = message;
    app.active_screen = Active::from_route(route, app.state.presentation());
    Task::none()
}
