use iced::{
    Element, Length, Subscription, Task,
    widget::{column, container, text, text_input},
};

use super::Screen;
use crate::ui::{
    message::{Message, Route},
    state::AppContext,
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
                //TODO: validate and test server url before commiting to config.
                ctx.config.server_url = self.server_url.clone();
                if let Err(e) = ctx.config.save() {
                    tracing::error!("Failed to save config: {}", e);
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
            text_input(&self.server_url, &self.server_url)
                .on_input(|val| Message::Onboarding(OnboardingMessage::ServerUrlChanged(val)))
        ]
        .spacing(20)
        .align_x(iced::Alignment::Center);

        container(content).center(Length::Fill).into()
    }
}
