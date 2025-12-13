use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, text, text_input},
};

use super::Screen;
use crate::ui::app::Message;

pub struct HomeScreen {
    pub local_id: Option<String>,
    pub remote_id_input: String,
}

impl HomeScreen {
    pub fn new() -> Self {
        Self { local_id: None, remote_id_input: String::new() }
    }
}

impl Screen for HomeScreen {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RemoteIdChanged(id) => {
                self.remote_id_input = id;
                Task::none()
            }
            Message::LocalIdFetched(id) => {
                self.local_id = Some(id);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let title = text("Welcome to Loki").size(30);

        let id_display = match &self.local_id {
            Some(id) => text(format!("My ID: {}", id)).size(20),
            None => text("Connecting to signaling server...").size(20),
        };

        let remote_input = text_input("Enter Peer ID to call", &self.remote_id_input)
            .on_input(Message::RemoteIdChanged)
            .padding(10)
            .width(Length::Fixed(400.0));

        let call_button = button("Call Peer")
            .on_press_maybe(if !self.remote_id_input.is_empty() {
                Some(Message::StartCall(self.remote_id_input.clone()))
            } else {
                None
            })
            .padding(10);

        let content = column![title, id_display, remote_input, call_button]
            .spacing(20)
            .align_x(iced::Alignment::Center);

        container(content).center(Length::Fill).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }
}
