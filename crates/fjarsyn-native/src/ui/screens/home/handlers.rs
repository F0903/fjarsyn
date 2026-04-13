use iced::Task;

use super::{HomeScreen, workflow};
use crate::ui::{
    message::{Message, ScreenMessage},
    shell::AppContextMut,
};

impl HomeScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        if let Message::Screen(ScreenMessage::Home(message)) = message {
            workflow::execute_home_message(self, message);
        }

        Task::none()
    }
}
