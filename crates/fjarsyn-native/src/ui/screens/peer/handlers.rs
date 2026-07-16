use iced::Task;

use super::{PeerMessage, PeerScreen};
use crate::ui::{
    message::{Message, PeerActionMessage, ScreenMessage},
    shell::ShellContextMut,
};

impl PeerScreen {
    pub(crate) fn handle_message(
        &mut self,
        ctx: &mut ShellContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        let Message::Screen(ScreenMessage::Peer(message)) = message else {
            return Task::none();
        };

        match message {
            PeerMessage::DraftChanged(value) => {
                self.draft = value;
                Task::none()
            }
            PeerMessage::ToggleLocalPreview => {
                self.local_preview_visible = !self.local_preview_visible;
                Task::none()
            }
            PeerMessage::SendPressed => {
                let body = self.draft.trim().to_owned();
                if body.is_empty() {
                    return Task::none();
                }
                let Some(session_id) = ctx.connected_session_id(&self.peer_id) else {
                    ctx.notify_error("Connect to this contact before sending a message.");
                    return Task::none();
                };
                self.draft.clear();
                Task::done(Message::PeerAction(PeerActionMessage::SendMessage {
                    session_id,
                    peer_id: self.peer_id.clone(),
                    body,
                }))
            }
        }
    }
}
