use std::sync::Arc;

use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, text, text_input},
};

use super::Screen;
use crate::{
    networking::webrtc::WebRTC,
    ui::{
        message::{Message, Route},
        screens::ScreenError,
        state::AppContext,
    },
};

#[derive(Debug, Clone)]
pub struct HomeScreen {}

impl HomeScreen {
    pub fn new(ctx: &mut AppContext) -> Result<(Self, Task<Message>), ScreenError> {
        let Some(frame_tx) = ctx.frame_tx.take() else {
            return Err(ScreenError::ScreenInitializationError(
                "Frame transmitter not found".to_owned(),
            ));
        };
        let Some(webrtc_event_tx) = ctx.webrtc_event_tx.take() else {
            return Err(ScreenError::ScreenInitializationError(
                "WebRTC event transmitter not found".to_owned(),
            ));
        };
        let server_url = ctx.config.server_url.clone();
        Ok((
            Self {},
            Task::future(async move {
                WebRTC::new(server_url, frame_tx, webrtc_event_tx).await.map_err(Arc::new)
            })
            .map(Message::WebRTCInitialized),
        ))
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::TargetIdChanged(id) => {
                ctx.target_id = Some(id);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self, ctx: &AppContext) -> Element<'_, Message> {
        let title = text("Welcome to Fjarsyn").size(30);

        let id_display = match &ctx.webrtc {
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
            text_input("Enter Peer ID to call", ctx.target_id.as_deref().unwrap_or(""))
                .on_input(Message::TargetIdChanged)
                .padding(10)
                .width(Length::Fixed(400.0));

        let call_button = button("Call Peer")
            .on_press_maybe(
                if let Some(id) = ctx.target_id.as_deref()
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
