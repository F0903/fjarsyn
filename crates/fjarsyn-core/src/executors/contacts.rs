use std::sync::Arc;

use super::{AppCommands, notify_error, notify_info, notify_success};
use crate::{
    app::{AppCommand, AppState},
    services::contacts_service::Contact,
};

#[derive(Debug, Clone)]
pub enum ContactsAction {
    LoadRequested,
    SaveRequested { peer_id: String, name: String, address: Option<String> },
    DeleteRequested(i64),
    UpdateAddressRequested { id: i64, new_address: String },
    Loaded(Result<Arc<Vec<Contact>>, String>),
    Saved(Result<Arc<Vec<Contact>>, String>),
    Deleted(Result<Arc<Vec<Contact>>, String>),
    Updated(Result<Arc<Vec<Contact>>, String>),
}

pub fn execute_contacts_action(state: &mut AppState, action: ContactsAction) -> AppCommands {
    match action {
        ContactsAction::LoadRequested => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            smallvec::smallvec![AppCommand::LoadContacts]
        }
        ContactsAction::SaveRequested { peer_id, name, address } => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            smallvec::smallvec![AppCommand::SaveContact { peer_id, name, address }]
        }
        ContactsAction::DeleteRequested(id) => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            smallvec::smallvec![AppCommand::DeleteContact { id }]
        }
        ContactsAction::UpdateAddressRequested { id, new_address } => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            let Some(contact) = state.contacts.contacts.iter().find(|contact| contact.id == id)
            else {
                return smallvec::smallvec![notify_error("Contact not found.")];
            };

            smallvec::smallvec![
                notify_info(format!("Updating address for {}...", contact.name)),
                AppCommand::UpdateContactAddress {
                    id: contact.id,
                    peer_id: contact.peer_id.clone(),
                    name: contact.name.clone(),
                    address: new_address,
                },
            ]
        }
        ContactsAction::Loaded(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                AppCommands::new()
            }
            Err(err) => {
                smallvec::smallvec![notify_error(format!("Unable to load contacts: {}", err))]
            }
        },
        ContactsAction::Saved(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                smallvec::smallvec![notify_success("Contact saved.")]
            }
            Err(err) => smallvec::smallvec![notify_error(format!("Save Failed: {}", err))],
        },
        ContactsAction::Deleted(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                smallvec::smallvec![notify_success("Contact deleted.")]
            }
            Err(err) => smallvec::smallvec![notify_error(format!("Delete Failed: {}", err))],
        },
        ContactsAction::Updated(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                smallvec::smallvec![notify_success("Contact updated.")]
            }
            Err(err) => smallvec::smallvec![notify_error(format!("Update Failed: {}", err))],
        },
    }
}

fn contacts_unavailable_message(state: &AppState) -> &'static str {
    if !state.accepts_user_requests() {
        "Contacts are unavailable while the app is shutting down."
    } else {
        "Contacts are unavailable until the database is ready."
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        app::{NotificationLevel, ServicePhase},
        executors::test_support::state,
    };

    #[test]
    fn contact_update_uses_current_contact_snapshot() {
        let mut state = state();
        state.services.database = ServicePhase::Ready;
        state.contacts.contacts = Arc::new(vec![Contact {
            id: 7,
            peer_id: "peer-a".into(),
            name: "Peer A".into(),
            address: None,
        }]);

        let commands = execute_contacts_action(
            &mut state,
            ContactsAction::UpdateAddressRequested { id: 7, new_address: "127.0.0.1:9000".into() },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::UpdateContactAddress { id, address, .. }
            if *id == 7 && address == "127.0.0.1:9000"
        )));
    }

    #[test]
    fn saving_contact_requires_ready_database() {
        let mut state = state();

        let commands = execute_contacts_action(
            &mut state,
            ContactsAction::SaveRequested {
                peer_id: "peer-a".into(),
                name: "Peer A".into(),
                address: None,
            },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::Notify { level: NotificationLevel::Error, .. }
        )));
    }
}
