use fjarsyn_engine::{
    identity::PeerId,
    messaging,
    peer_session::{self, SessionId},
};
use iced::Task;

use super::runtime_unavailable;
use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

pub(super) fn send(
    app: &mut Fjarsyn,
    session_id: SessionId,
    peer_id: PeerId,
    body: String,
) -> Task<Message> {
    let Some(messaging) =
        app.runtime.application.as_ref().map(|runtime| runtime.messaging().clone())
    else {
        return runtime_unavailable(app);
    };
    Task::future(async move {
        let outcome = match messaging.send_message(session_id, peer_id, body).await {
            Ok(_) => message::peer::SendOutcome::Sent,
            Err(messaging::Error::Session(peer_session::Error::OutcomeUnknown)) => {
                message::peer::SendOutcome::DeliveryUncertain
            }
            Err(error) => message::peer::SendOutcome::Failed(error.to_string()),
        };
        Message::PeerAction(message::peer::Action::MessageSent(outcome))
    })
}

pub(super) fn finish_send(app: &mut Fjarsyn, outcome: message::peer::SendOutcome) -> Task<Message> {
    match outcome {
        message::peer::SendOutcome::Sent => {}
        message::peer::SendOutcome::DeliveryUncertain => app
            .state
            .notify_info("Delivery could not be confirmed; the message is marked uncertain."),
        message::peer::SendOutcome::Failed(error) => {
            app.state.notify_error(format!("Message failed: {error}"));
        }
    }
    Task::none()
}
