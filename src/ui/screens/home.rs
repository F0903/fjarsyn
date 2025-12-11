use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, text},
};

use super::Screen;
use crate::ui::app::{Message, Route};

pub struct HomeScreen;

impl HomeScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for HomeScreen {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(_) => unreachable!(), // Navigate is handled by App
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        container(
            column![
                text("Welcome to Loki").size(30),
                text("(this is a placeholder screen)").size(10),
                button("Go to Capture Screen")
                    .on_press(Message::Navigate(Route::Capture))
                    .padding(10)
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }
}
