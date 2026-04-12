use iced::{Subscription, Task};

use crate::ui::{
    app::{AppContext, AppContextMut},
    message::Message,
    screens::Screen,
};

mod handlers;
mod view;
mod workflow;

#[derive(Debug, Clone)]
pub enum MessagesMessage {
    DraftChanged(String),
    SendPressed,
    ClearDraft(String),
}

#[derive(Debug, Clone)]
pub struct MessagesScreen {
    pub(crate) selected_peer_id: Option<String>,
    pub(crate) draft: String,
}

impl MessagesScreen {
    pub fn new(ctx: AppContext<'_>, selected_peer_id: Option<String>) -> Self {
        let selected_peer_id = resolve_selected_peer_id(ctx, selected_peer_id);

        Self { selected_peer_id, draft: String::new() }
    }
}

impl Screen for MessagesScreen {
    fn subscription(&self, _ctx: AppContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: AppContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}

pub(crate) fn resolve_selected_peer_id(
    ctx: AppContext<'_>,
    selected_peer_id: Option<String>,
) -> Option<String> {
    fjarsyn_core::app::resolve_selected_peer_id(&ctx.messaging.summaries, selected_peer_id)
}
