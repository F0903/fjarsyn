use iced::Task;

use super::{HomeMessage, HomeScreen};
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
        match message {
            Message::Screen(ScreenMessage::Home(msg)) => match msg {
                HomeMessage::TargetAddressChanged(val) => {
                    self.manual_target_address = val;
                    Task::none()
                }
            },
            _ => Task::none(),
        }
    }
}
