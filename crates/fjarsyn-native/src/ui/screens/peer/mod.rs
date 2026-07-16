mod handlers;
mod view;

use fjarsyn_core::peer_session::PeerId;
use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{ShellContext, ShellContextMut},
};

#[derive(Debug, Clone)]
pub enum PeerMessage {
    DraftChanged(String),
    SendPressed,
    ToggleLocalPreview,
}

/// Presentation state for one stable peer route.
#[derive(Debug, Clone)]
pub struct PeerScreen {
    peer_id: PeerId,
    pub(crate) draft: String,
    pub(crate) local_preview_visible: bool,
}

impl PeerScreen {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id, draft: String::new(), local_preview_visible: true }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }
}

impl Screen for PeerScreen {
    fn subscription(&self, _ctx: ShellContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut ShellContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: ShellContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
