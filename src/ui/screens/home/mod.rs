use iced::{Subscription, Task};

use crate::ui::{app::AppContext, message::Message, screens::Screen};

mod view;

#[derive(Debug, Clone)]
pub enum HomeMessage {
    TargetAddressChanged(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {
    pub(crate) manual_target_address: String,
}

impl HomeScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self { manual_target_address: String::new() }
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::Home(msg) => match msg {
                HomeMessage::TargetAddressChanged(val) => {
                    self.manual_target_address = val;
                    Task::none()
                }
            },
            _ => Task::none(),
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
