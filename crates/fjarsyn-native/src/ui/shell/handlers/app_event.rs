use fjarsyn_core::executors::{self, AppEvent};
use iced::Task;

use super::app_command;
use crate::ui::{message::Message, shell::Fjarsyn};

pub fn execute_app_event(app: &mut Fjarsyn, event: AppEvent) -> Task<Message> {
    let commands = executors::execute_app_event(&mut app.ctx.core, event);
    app_command::run_app_commands(app, commands)
}
