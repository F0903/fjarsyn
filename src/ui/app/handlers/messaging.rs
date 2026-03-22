use iced::Task;

use crate::ui::{
    app::{
        Fjarsyn,
        workflows::messaging::{self, MessagingEffect},
    },
    message::{Message, MessagingServiceMessage, NavigationMessage, ScreenMessage},
    screens::messages::MessagesMessage,
    utils::ErrorExt,
};

pub fn handle_messaging_msg(app: &mut Fjarsyn, message: MessagingServiceMessage) -> Task<Message> {
    let effects = messaging::reduce(app, message);
    Task::batch(effects.into_iter().map(run_effect))
}

fn run_effect(effect: MessagingEffect) -> Task<Message> {
    match effect {
        MessagingEffect::NotifyError(message) => Task::done(Message::Notification(
            crate::ui::message::NotificationMessage::NotifyError(message),
        )),
        MessagingEffect::NotifyInfo(message) => Task::done(Message::Notification(
            crate::ui::message::NotificationMessage::NotifyInfo(message),
        )),
        MessagingEffect::Navigate(route) => {
            Task::done(Message::Navigation(NavigationMessage::Navigate(route)))
        }
        MessagingEffect::SendMessage { service, peer_id, address, body } => {
            Task::future(async move {
                service
                    .send_message(peer_id.clone(), address, body)
                    .await
                    .map(|peer_id| {
                        Message::Screen(ScreenMessage::Messages(MessagesMessage::ClearDraft(
                            peer_id,
                        )))
                    })
                    .unwrap_or_else(|err| err.to_notify_error())
            })
        }
    }
}
