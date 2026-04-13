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
pub enum HomeMessage {
    TargetAddressChanged(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {
    pub(crate) manual_target_address: String,
}

impl HomeScreen {
    pub fn new(_ctx: AppContext<'_>) -> Self {
        Self { manual_target_address: String::new() }
    }
}

impl Screen for HomeScreen {
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
