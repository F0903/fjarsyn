use iced::{
    Element, Length, Subscription, Task,
    widget::{column, container, text},
};

use super::Screen;
use crate::ui::{message::Message, state::AppContext};

#[derive(Debug, Clone)]
pub enum ContactsMessage {
    NoOp,
}

#[derive(Debug, Clone)]
pub struct ContactsScreen {}

impl ContactsScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self {}
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut AppContext, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn view<'a>(&'a self, _ctx: &'a AppContext) -> Element<'a, Message> {
        let title = text("Contacts").size(30);
        let placeholder = text("Manage your contacts here. (Work in progress)");

        let content = column![title, placeholder].spacing(20).align_x(iced::Alignment::Center);

        container(content).center(Length::Fill).into()
    }
}
