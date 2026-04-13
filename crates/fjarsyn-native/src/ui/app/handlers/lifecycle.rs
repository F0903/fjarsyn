use fjarsyn_core::executors::{AppEvent, LifecycleAction};
use iced::Task;

use super::app_event;
use crate::ui::{
    app::Fjarsyn,
    message::{LifecycleMessage, Message},
};

pub fn handle_lifecycle_msg(app: &mut Fjarsyn, message: LifecycleMessage) -> Task<Message> {
    let event = match message {
        LifecycleMessage::RetryStartup => {
            AppEvent::Lifecycle(LifecycleAction::RetryStartupRequested)
        }
    };

    app_event::execute_app_event(app, event)
}
