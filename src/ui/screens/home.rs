use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, row, text, text_input},
};

use super::Screen;
use crate::ui::{
    message::{Message, Route},
    state::AppContext,
};

#[derive(Debug, Clone)]
pub enum HomeMessage {
    TargetIdChanged(String),
    StartCall(String),
    CopyId(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {}

impl HomeScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self {}
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::Home(msg) => match msg {
                HomeMessage::TargetIdChanged(id) => {
                    ctx.target_id = Some(id);
                    Task::none()
                }
                HomeMessage::StartCall(target_id) => {
                    if let Some(webrtc) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        Task::future(async move {
                            match webrtc_clone.create_offer(target_id).await {
                                Ok(_) => Message::Navigate(Route::Call),
                                Err(e) => {
                                    tracing::error!("Failed to create offer: {}", e);
                                    Message::NoOp
                                }
                            }
                        })
                    } else {
                        tracing::warn!("Could not start call. WebRTC not initialized...");
                        Task::none()
                    }
                }
                HomeMessage::CopyId(id) => iced::clipboard::write(id),
            },
            _ => Task::none(),
        }
    }

    fn view(&self, ctx: &AppContext) -> Element<'_, Message> {
        let title = text("Welcome to Fjarsyn").size(30);

        let id_display = match &ctx.webrtc {
            Some(webrtc) => match webrtc.get_local_id() {
                Some(id) => row![
                    text(format!("My ID: {}", id)).size(20),
                    button("Copy").on_press(Message::Home(HomeMessage::CopyId(id)))
                ]
                .spacing(10),
                None => row![text("Connecting to signaling server...").size(20)],
            },
            None => row![text("Connecting to signaling server...").size(20)],
        };

        let remote_input =
            text_input("Enter Peer ID to call", ctx.target_id.as_deref().unwrap_or(""))
                .on_input(|id| Message::Home(HomeMessage::TargetIdChanged(id)))
                .padding(10)
                .width(Length::Fixed(400.0));

        let call_button = button("Call Peer")
            .on_press_maybe(
                if let Some(id) = ctx.target_id.as_deref()
                    && !id.is_empty()
                {
                    Some(Message::Home(HomeMessage::StartCall(id.to_owned())))
                } else {
                    None
                },
            )
            .padding(10);

        let settings_button =
            button("Settings").on_press(Message::NavigateWithBack(Route::Settings)).padding(10);

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
