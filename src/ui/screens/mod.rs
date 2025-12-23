pub mod call;
pub mod home;
pub mod onboarding;
pub mod settings;

use iced::{Element, Subscription, Task};

use crate::ui::{app::App, message::Message, state::AppContext};

#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("Screen initialization error: {0}")]
    ScreenInitializationError(String),
}

pub trait Screen {
    fn update(
        &mut self,
        ctx: &mut AppContext,
        message: <App as iced::Program>::Message,
    ) -> Task<Message>;
    fn view(&self, ctx: &AppContext) -> Element<'_, Message>;
    fn subscription(&self, ctx: &AppContext) -> Subscription<Message>;
}
