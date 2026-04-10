use iced::{Subscription, Task};

use crate::ui::{app::AppState, message::Message, screens::Screen};

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
    pub fn new(ctx: &mut AppState, selected_peer_id: Option<String>) -> Self {
        let selected_peer_id = selected_peer_id.or_else(|| first_peer_id(ctx));

        Self { selected_peer_id, draft: String::new() }
    }
}

impl Screen for MessagesScreen {
    fn subscription(&self, _ctx: &AppState) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppState, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: &'a AppState) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}

fn first_peer_id(ctx: &AppState) -> Option<String> {
    if let Some(message) = ctx.messaging.messages.iter().last() {
        return Some(message.peer_id.clone());
    }

    None
}
