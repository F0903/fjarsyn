use std::future::Future;

use fjarsyn_engine::{
    identity::PeerId,
    peer_session::{ServiceHandle, SessionId},
};
use iced::Task;

use super::{runtime_unavailable, session_handle};
use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

pub(super) fn connect(app: &mut Fjarsyn, peer_id: PeerId) -> Task<Message> {
    let Some(sessions) = session_handle(app) else {
        return runtime_unavailable(app);
    };
    Task::future(async move {
        Message::PeerAction(message::peer::Action::ConnectCompleted(
            sessions.connect(peer_id).await.map_err(|error| error.to_string()),
        ))
    })
}

pub(super) fn finish_connect(
    app: &mut Fjarsyn,
    result: Result<SessionId, String>,
) -> Task<Message> {
    if let Err(error) = result {
        app.state.notify_error(format!("Connection failed: {error}"));
    }
    Task::none()
}

pub(super) fn accept(app: &mut Fjarsyn, session_id: SessionId) -> Task<Message> {
    session_command(app, move |sessions| async move {
        sessions.accept(session_id).await.map_err(|error| error.to_string())
    })
}

pub(super) fn reject(app: &mut Fjarsyn, session_id: SessionId) -> Task<Message> {
    session_command(app, move |sessions| async move {
        sessions.reject(session_id, "rejected by user").await.map_err(|error| error.to_string())
    })
}

pub(super) fn disconnect(app: &mut Fjarsyn, session_id: SessionId) -> Task<Message> {
    session_command(app, move |sessions| async move {
        sessions.disconnect(session_id).await.map_err(|error| error.to_string())
    })
}

pub(super) fn finish_command(app: &mut Fjarsyn, result: Result<(), String>) -> Task<Message> {
    if let Err(error) = result {
        app.state.notify_error(error);
    }
    Task::none()
}

fn session_command<F, Fut>(app: &mut Fjarsyn, command: F) -> Task<Message>
where
    F: FnOnce(ServiceHandle) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    let Some(sessions) = session_handle(app) else {
        return runtime_unavailable(app);
    };
    Task::future(async move {
        Message::PeerAction(message::peer::Action::SessionCommandCompleted(command(sessions).await))
    })
}
