use iced::Task;

use super::{HomeMessage, HomeScreen};
use crate::ui::{app::AppContext, message::Message};

impl HomeScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppContext,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::Home(msg) => match msg {
                HomeMessage::TargetAddressChanged(val) => {
                    self.manual_target_address = val;
                    Task::none()
                }
            },
            _ => Task::none(),
        }
    }
}
