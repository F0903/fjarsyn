mod call;
mod config;
mod contacts;
mod lifecycle;
mod messaging;
mod service;
#[cfg(test)]
pub(crate) mod test_support;

pub use call::{CallAction, execute_call_action};
pub use config::{ConfigAction, execute_config_action};
pub use contacts::{ContactsAction, execute_contacts_action};
pub use lifecycle::{LifecycleAction, execute_lifecycle_action};
pub use messaging::{MessagingAction, execute_messaging_action};
pub use service::{ServiceAction, execute_service_action};
use smallvec::SmallVec;

use crate::app::{AppCommand, AppLifecycle, AppState, NotificationLevel, recompute_lifecycle};

pub type AppCommands = SmallVec<[AppCommand; 4]>;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Lifecycle(LifecycleAction),
    Config(ConfigAction),
    Contacts(ContactsAction),
    Messaging(MessagingAction),
    Call(CallAction),
    Service(ServiceAction),
}

pub fn execute_app_event(state: &mut AppState, event: AppEvent) -> AppCommands {
    if matches!(state.lifecycle, AppLifecycle::ShuttingDown)
        && !matches!(event, AppEvent::Lifecycle(_))
    {
        return AppCommands::new();
    }

    let commands = match event {
        AppEvent::Lifecycle(action) => execute_lifecycle_action(state, action),
        AppEvent::Config(action) => execute_config_action(state, action),
        AppEvent::Contacts(action) => execute_contacts_action(state, action),
        AppEvent::Messaging(action) => execute_messaging_action(state, action),
        AppEvent::Call(action) => execute_call_action(state, action),
        AppEvent::Service(action) => execute_service_action(state, action),
    };

    recompute_lifecycle(state);
    commands
}

pub(crate) fn notify_error(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Error, message: message.into() }
}

pub(crate) fn notify_info(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Info, message: message.into() }
}

pub(crate) fn notify_success(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Success, message: message.into() }
}
