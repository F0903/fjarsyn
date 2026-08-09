use fjarsyn_engine::identity::PeerId;
use iced::Task;

use crate::ui::{
    message::{self, Message, screen::PeerMessage},
    presentation::Context,
};

/// Presentation state for one stable peer route.
#[derive(Debug, Clone)]
pub(in crate::ui::screens) struct Screen {
    pub(super) peer_id: PeerId,
    pub(super) draft: String,
    pub(super) local_preview_visible: bool,
}

impl Screen {
    pub(in crate::ui::screens) fn new(peer_id: PeerId) -> Self {
        Self { peer_id, draft: String::new(), local_preview_visible: true }
    }

    pub(in crate::ui::screens) fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub(in crate::ui::screens) fn update(
        &mut self,
        context: Context<'_>,
        message: PeerMessage,
    ) -> Task<Message> {
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
                let Some(session_id) = context.connected_session_id(&self.peer_id) else {
                    return Task::done(Message::Notification(message::Notification::NotifyError(
                        "Connect to this contact before sending a message.".into(),
                    )));
                };
                self.draft.clear();
                Task::done(Message::PeerAction(message::peer::Action::SendMessage {
                    session_id,
                    peer_id: self.peer_id.clone(),
                    body,
                }))
            }
        }
    }
}
