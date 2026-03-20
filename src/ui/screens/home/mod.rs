use iced::{Subscription, Task};

use crate::ui::{app::AppState, message::Message, screens::Screen};

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
    pub fn new(_ctx: &mut AppState) -> Self {
        Self { manual_target_address: String::new() }
    }
}

impl Screen for HomeScreen {
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
