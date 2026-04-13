use fjarsyn_core::executors::{AppEvent, ConfigAction};
use iced::Task;

use super::app_event;
use crate::ui::{
    message::{ConfigMessage, Message},
    shell::Fjarsyn,
};

pub fn handle_config_msg(app: &mut Fjarsyn, message: ConfigMessage) -> Task<Message> {
    match message {
        ConfigMessage::SaveRequested(config) => {
            app_event::execute_app_event(app, AppEvent::Config(ConfigAction::SaveRequested(config)))
        }
        ConfigMessage::CaptureReadbackApplied => Task::none(),
    }
}
