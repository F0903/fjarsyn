use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, text, text_input},
};

use super::Screen;
use crate::ui::{
    message::{Message, Route},
    state::State,
};

#[derive(Debug, Clone)]
pub struct HomeScreen {}

impl HomeScreen {
    pub fn new() -> Self {
        Self {}
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _state: &State) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&self, state: &mut State, message: Message) -> Task<Message> {
        match message {
            Message::TargetIdChanged(id) => {
                state.target_id = Some(id);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self, state: &State) -> Element<'_, Message> {
        let title = text("Welcome to Fjarsyn").size(30);

        let id_display = match &state.webrtc {
            Some(Ok(webrtc)) => match webrtc.get_local_id() {
                Some(id) => row![
                    text(format!("My ID: {}", id)).size(20),
                    button("Copy").on_press(Message::CopyId(id))
                ]
                .spacing(10),
                None => row![text("Connecting to signaling server...").size(20)],
            },
            Some(Err(err)) => row![text(format!("Error: {}", err)).size(20)],
            None => row![text("Connecting to signaling server...").size(20)],
        };

        let remote_input =
            text_input("Enter Peer ID to call", state.target_id.as_deref().unwrap_or(""))
                .on_input(Message::TargetIdChanged)
                .padding(10)
                .width(Length::Fixed(400.0));

        let call_button = button("Call Peer")
            .on_press_maybe(
                if let Some(id) = state.target_id.as_deref()
                    && !id.is_empty()
                {
                    Some(Message::StartCall(id.to_owned()))
                } else {
                    None
                },
            )
            .padding(10);

        let settings_button =
            button("Settings").on_press(Message::Navigate(Route::Settings)).padding(10);

        let content = column![
            title,
            id_display,
            remote_input,
            row![call_button, settings_button].spacing(20)
        ]
        .spacing(20)
        .align_x(iced::Alignment::Center);

        container(content).center(Length::Fill).into()
    }
}
