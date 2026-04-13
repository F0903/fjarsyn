use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{AppContext, AppContextMut},
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
    pub(crate) draft: String,
}

impl MessagesScreen {
    pub fn new() -> Self {
        Self { draft: String::new() }
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
