use fjarsyn_core::executors::{AppEvent, CallAction};
use iced::Task;

use crate::ui::{
    app::{Fjarsyn, handlers::app_event},
    message::{CallActionMessage, Message},
};

pub fn handle_call_action_msg(app: &mut Fjarsyn, message: CallActionMessage) -> Task<Message> {
    let action = match message {
        CallActionMessage::AcceptCall => CallAction::AcceptRequested,
        CallActionMessage::AcceptFailed { error, peer_id } => {
            CallAction::AcceptFailed { error, peer_id }
        }
        CallActionMessage::DeclineCall => CallAction::DeclineRequested,
        CallActionMessage::DeclineFailed { error, peer_id } => {
            CallAction::DeclineFailed { error, peer_id }
        }
        CallActionMessage::StartCall(target) => CallAction::StartRequested(target),
        CallActionMessage::StartFailed(error) => CallAction::StartFailed(error),
    };

    app_event::execute_app_event(app, AppEvent::Call(action))
}
