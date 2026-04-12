use iced::Task;

use super::{HomeScreen, workflow};
use crate::ui::{
    app::AppContextMut,
    message::{Message, ScreenMessage},
};

impl HomeScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        if let Message::Screen(ScreenMessage::Home(message)) = message {
            workflow::reduce(self, message);
        }

        Task::none()
    }
}
