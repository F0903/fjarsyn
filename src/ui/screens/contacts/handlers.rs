use iced::Task;

use super::{ContactsScreen, workflow};
use crate::ui::{
    app::AppState,
    message::{Message, ScreenMessage},
};

impl ContactsScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppState,
        message: Message,
    ) -> Task<Message> {
        let effects = match message {
            Message::Screen(ScreenMessage::Contacts(message)) => workflow::reduce(self, message),
            _ => return Task::none(),
        };

        Task::batch(
            effects
                .into_iter()
                .map(|effect| Task::done(Message::ContactData(workflow::into_message(effect)))),
        )
    }
}
