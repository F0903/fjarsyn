//! Peer-session, messaging, and screen-share action dispatch.

use fjarsyn_engine::peer_session;
use iced::Task;

use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

mod messaging;
mod screen_share;
mod session;

pub(in crate::ui::shell) fn handle_peer_action(
    app: &mut Fjarsyn,
    action: message::peer::Action,
) -> Task<Message> {
    match action {
        message::peer::Action::Connect(peer_id) => session::connect(app, peer_id),
        message::peer::Action::ConnectCompleted(result) => session::finish_connect(app, result),
        message::peer::Action::Accept { session_id } => session::accept(app, session_id),
        message::peer::Action::Reject { session_id } => session::reject(app, session_id),
        message::peer::Action::Disconnect { session_id } => session::disconnect(app, session_id),
        message::peer::Action::SessionCommandCompleted(result) => {
            session::finish_command(app, result)
        }
        message::peer::Action::SendMessage { session_id, peer_id, body } => {
            messaging::send(app, session_id, peer_id, body)
        }
        message::peer::Action::MessageSent(outcome) => messaging::finish_send(app, outcome),
        message::peer::Action::BeginScreenShare { session_id } => {
            screen_share::begin(app, session_id)
        }
        message::peer::Action::CaptureSourceSelected { selection, result } => {
            screen_share::capture_source_selected(app, selection, result)
        }
        message::peer::Action::StopScreenShare { session_id } => {
            screen_share::stop(app, session_id)
        }
        message::peer::Action::ScreenShareCompleted(result) => screen_share::finish(app, result),
    }
}

fn session_handle(app: &Fjarsyn) -> Option<peer_session::ServiceHandle> {
    app.runtime.engine.as_ref().map(|runtime| runtime.sessions().clone())
}

fn runtime_unavailable(app: &mut Fjarsyn) -> Task<Message> {
    app.state.notify_error("Peer services are unavailable while Fjarsyn is starting.");
    Task::none()
}
