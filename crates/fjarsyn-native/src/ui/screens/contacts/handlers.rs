use iced::Task;

use super::{ContactsScreen, workflow};
use crate::ui::{
    message::{Message, ScreenMessage},
    shell::AppContextMut,
};

impl ContactsScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        let effects = match message {
            Message::Screen(ScreenMessage::Contacts(message)) => {
                workflow::execute_contacts_message(self, message)
            }
            _ => return Task::none(),
        };

        Task::batch(
            effects
                .into_iter()
                .map(|effect| Task::done(Message::ContactData(workflow::into_message(effect)))),
        )
    }
}
