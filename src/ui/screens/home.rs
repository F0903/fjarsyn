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
pub enum HomeMessage {
    TargetIdChanged(String),
    StartCall(String),
    CopyId(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {}

impl HomeScreen {
    pub fn new(ctx: &mut AppContext) -> Result<(Self, Task<Message>), ScreenError> {
        let mut task = Task::none();

        // We init WebRTC here if it's not already initialized
        if ctx.webrtc.is_none() {
            let Some(frame_tx) = ctx.frame_tx.clone() else {
                return Err(ScreenError::ScreenInitializationError(
                    "Frame channel not initialized.".to_owned(),
                ));
            };
            let Some(webrtc_event_tx) = ctx.webrtc_event_tx.clone() else {
                return Err(ScreenError::ScreenInitializationError(
                    "WebRTC event channel not initialized.".to_owned(),
                ));
            };
            let server_url = ctx.config.server_url.clone();
            task = Task::future(
                async move { WebRTC::new(server_url, frame_tx, webrtc_event_tx).await },
            )
            .map_err(std::sync::Arc::new)
            .map(Message::WebRTCInitialized);
        }

        Ok((Self {}, task))
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
                    if let Some(Ok(webrtc)) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        Task::future(async move {
                            match webrtc_clone.create_offer(target_id).await {
                                Ok(_) => Message::Navigate(Route::Capture),
                                Err(e) => Message::Error(format!("Failed to create offer: {}", e)),
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
            Some(Ok(webrtc)) => match webrtc.get_local_id() {
                Some(id) => row![
                    text(format!("My ID: {}", id)).size(20),
                    button("Copy").on_press(Message::Home(HomeMessage::CopyId(id)))
                ]
                .spacing(10),
                None => row![text("Connecting to signaling server...").size(20)],
            },
            Some(Err(err)) => row![text(format!("Error: {}", err)).size(20)],
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
