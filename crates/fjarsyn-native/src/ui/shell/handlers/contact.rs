use fjarsyn_core::executors::{AppEvent, ContactsAction};
use iced::Task;

use crate::ui::{
    message::{ContactsServiceMessage, Message},
    shell::{Fjarsyn, handlers::app_event},
};

pub fn handle_contact_msg(app: &mut Fjarsyn, message: ContactsServiceMessage) -> Task<Message> {
    let action = match message {
        ContactsServiceMessage::LoadContacts => ContactsAction::LoadRequested,
        ContactsServiceMessage::SaveContact { peer_id, name, address, trusted_public_key } => {
            ContactsAction::SaveRequested { peer_id, name, address, trusted_public_key }
        }
        ContactsServiceMessage::DeleteContact(id) => ContactsAction::DeleteRequested(id),
        ContactsServiceMessage::UpdateContactAddress { id, new_address } => {
            ContactsAction::UpdateAddressRequested { id, new_address }
        }
        ContactsServiceMessage::UpdateContactTrustedPublicKey { id, trusted_public_key } => {
            ContactsAction::UpdateTrustedPublicKeyRequested { id, trusted_public_key }
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

    let task = app_event::execute_app_event(app, AppEvent::Contacts(action));
    sync_trusted_signaling_peers(app);
    task
}

fn sync_trusted_signaling_peers(app: &Fjarsyn) {
    let Some(call_service) = app.runtime.services.call_service.as_ref() else {
        return;
    };

    call_service.replace_trusted_contacts(app.ctx.core.contacts.contacts.iter());
}
