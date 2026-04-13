use fjarsyn_core::executors::{AppEvent, ContactsAction};
use iced::Task;

use crate::ui::{
    message::{ContactsServiceMessage, Message},
    shell::{Fjarsyn, handlers::app_event},
};

pub fn handle_contact_msg(app: &mut Fjarsyn, message: ContactsServiceMessage) -> Task<Message> {
    let action = match message {
        ContactsServiceMessage::LoadContacts => ContactsAction::LoadRequested,
        ContactsServiceMessage::SaveContact { peer_id, name, address } => {
            ContactsAction::SaveRequested { peer_id, name, address }
        }
        ContactsServiceMessage::DeleteContact(id) => ContactsAction::DeleteRequested(id),
        ContactsServiceMessage::UpdateContactAddress { id, new_address } => {
            ContactsAction::UpdateAddressRequested { id, new_address }
        }
        ContactsServiceMessage::ContactsLoaded(result) => {
            ContactsAction::Loaded(result.map_err(|err| err.to_string()))
        }
        ContactsServiceMessage::ContactSaved(result) => {
            ContactsAction::Saved(result.map_err(|err| err.to_string()))
        }
        ContactsServiceMessage::ContactDeleted(result) => {
            ContactsAction::Deleted(result.map_err(|err| err.to_string()))
        }
        ContactsServiceMessage::ContactUpdated(result) => {
            ContactsAction::Updated(result.map_err(|err| err.to_string()))
        }
    };

    app_event::execute_app_event(app, AppEvent::Contacts(action))
}
