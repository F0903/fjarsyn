use fjarsyn_core::app::{self, CallAction};
use iced::Task;

use crate::ui::{
    app::{Fjarsyn, handlers::app_command},
    message::{CallActionMessage, Message},
};

pub fn handle_call_action_msg(app: &mut Fjarsyn, message: CallActionMessage) -> Task<Message> {
    let action = match message {
        CallActionMessage::AcceptCall => CallAction::AcceptRequested,
        CallActionMessage::DeclineCall => CallAction::DeclineRequested,
        CallActionMessage::StartCall(target) => CallAction::StartRequested(target),
    };

    let commands = app::reduce_call_action(&mut app.ctx.core, action);
    app_command::run_app_commands(app, commands)
}
