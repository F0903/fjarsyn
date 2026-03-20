use iced::Task;

use super::{HomeScreen, workflow};
use crate::ui::{
    app::AppState,
    message::{Message, ScreenMessage},
};

impl HomeScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppState,
        message: Message,
    ) -> Task<Message> {
        if let Message::Screen(ScreenMessage::Home(message)) = message {
            workflow::reduce(self, message);
        }

        Task::none()
    }
}
