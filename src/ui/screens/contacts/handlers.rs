use iced::Task;

use super::{ContactsMessage, ContactsScreen};
use crate::ui::{
    app::AppState,
    message::{ContactsServiceMessage, Message, ScreenMessage},
};

impl ContactsScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppState,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::Screen(ScreenMessage::Contacts(msg)) => match msg {
                ContactsMessage::NameChanged(val) => {
                    self.new_contact_name = val;
                    Task::none()
                }
                ContactsMessage::PeerIdChanged(val) => {
                    self.new_contact_peer_id = val;
                    Task::none()
                }
                ContactsMessage::AddressChanged(val) => {
                    self.new_contact_address = val;
                    Task::none()
                }
                ContactsMessage::ToggleAddForm => {
                    self.show_add_form = !self.show_add_form;
                    Task::none()
                }
                ContactsMessage::AddNewContact => {
                    let peer_id = self.new_contact_peer_id.trim().to_string();
                    let name = self.new_contact_name.trim().to_string();
                    let address = if self.new_contact_address.trim().is_empty() {
                        None
                    } else {
                        Some(self.new_contact_address.trim().to_string())
                    };

                    if peer_id.is_empty() || name.is_empty() {
                        return Task::none();
                    }

                    self.new_contact_name.clear();
                    self.new_contact_peer_id.clear();
                    self.new_contact_address.clear();
                    self.show_add_form = false;

                    Task::done(Message::ContactData(ContactsServiceMessage::SaveContact {
                        peer_id,
                        name,
                        address,
                    }))
                }
            },
            _ => Task::none(),
        }
    }
}
