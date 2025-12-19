use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, text, text_input},
};

use super::Screen;
use crate::{
    networking::webrtc::WebRTC,
    ui::{
        message::{Message, Route},
        state::AppContext,
    },
};

#[derive(Debug, Clone)]
pub enum OnboardingMessage {
    ServerUrlChanged(String),
    SaveClicked,
}

#[derive(Debug, Clone)]
pub struct OnboardingScreen {
    server_url: String,
}

impl OnboardingScreen {
    pub fn new(server_url: String) -> Self {
        Self { server_url }
    }
}

impl Screen for OnboardingScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::Onboarding(OnboardingMessage::ServerUrlChanged(url)) => {
                self.server_url = url;
                Task::none()
            }
            Message::Onboarding(OnboardingMessage::SaveClicked) => {
                let Some(frame_tx) = ctx.frame_tx.clone() else {
                    tracing::error!("Frame channel not available.");
                    return Task::none();
                };
                let Some(webrtc_event_tx) = ctx.webrtc_event_tx.clone() else {
                    tracing::error!("WebRTC event channel not available.");
                    return Task::none();
                };
                let server_url = ctx.config.server_url.clone();

                Task::future(
                    async move { WebRTC::new(server_url, frame_tx, webrtc_event_tx).await },
                )
                .map_err(std::sync::Arc::new)
                .map(Message::WebRTCInitialized)
            }

            Message::WebRTCInitialized(Ok(_webrtc)) => {
                ctx.config.onboarding_done = true;
                ctx.config.server_url = self.server_url.clone();
                if let Err(err) = ctx.config.save() {
                    tracing::error!("Failed to save config: {}", err);
                }
                Task::done(Message::Navigate(Route::Home))
            }

            _ => Task::none(),
        }
    }

    fn view(&self, _ctx: &AppContext) -> Element<'_, Message> {
        let content = column![
            text("Welcome to Fjarsyn").size(30),
            text("Before we get started, enter the URL of your signaling server").size(14),
            text_input("Signaling Server URL", &self.server_url)
                .on_input(|val| Message::Onboarding(OnboardingMessage::ServerUrlChanged(val)))
                .padding(10),
            button("Save").on_press(Message::Onboarding(OnboardingMessage::SaveClicked))
        ]
        .spacing(20)
        .align_x(iced::Alignment::Center)
        .max_width(500);

        container(content).center(Length::Fill).into()
    }
}
