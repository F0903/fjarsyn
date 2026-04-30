use std::sync::Arc;

use super::{AppCommands, notify_error, notify_info, notify_success};
use crate::{
    app::{AppCommand, AppState},
    networking::signaling::auth::TrustedPeerIdentity,
    services::contacts_service::Contact,
};

#[derive(Debug, Clone)]
pub enum ContactsAction {
    LoadRequested,
    SaveRequested {
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    },
    DeleteRequested(i64),
    UpdateAddressRequested {
        id: i64,
        new_address: String,
    },
    UpdateTrustedPublicKeyRequested {
        id: i64,
        trusted_public_key: String,
    },
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
        ContactsAction::SaveRequested { peer_id, name, address, trusted_public_key } => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            let trusted_public_key =
                match validate_required_trusted_public_key(&peer_id, trusted_public_key) {
                    Ok(trusted_public_key) => trusted_public_key,
                    Err(err) => return smallvec::smallvec![notify_error(err)],
                };

            smallvec::smallvec![AppCommand::SaveContact {
                peer_id,
                name,
                address,
                trusted_public_key: Some(trusted_public_key),
            }]
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
                AppCommand::UpdateContact {
                    id: contact.id,
                    peer_id: contact.peer_id.clone(),
                    name: contact.name.clone(),
                    address: Some(new_address),
                    trusted_public_key: contact.trusted_public_key.clone(),
                },
            ]
        }
        ContactsAction::UpdateTrustedPublicKeyRequested { id, trusted_public_key } => {
            if !state.can_use_contacts() {
                return smallvec::smallvec![notify_error(contacts_unavailable_message(state))];
            }

            let Some(contact) = state.contacts.contacts.iter().find(|contact| contact.id == id)
            else {
                return smallvec::smallvec![notify_error("Contact not found.")];
            };

            let trusted_public_key =
                match validate_trusted_public_key(&contact.peer_id, trusted_public_key) {
                    Ok(trusted_public_key) => trusted_public_key,
                    Err(err) => return smallvec::smallvec![notify_error(err)],
                };

            smallvec::smallvec![
                notify_info(format!("Updating trusted key for {}...", contact.name)),
                AppCommand::UpdateContact {
                    id: contact.id,
                    peer_id: contact.peer_id.clone(),
                    name: contact.name.clone(),
                    address: contact.address.clone(),
                    trusted_public_key: Some(trusted_public_key),
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

fn validate_required_trusted_public_key(
    peer_id: &str,
    trusted_public_key: Option<String>,
) -> Result<String, String> {
    let Some(trusted_public_key) = trusted_public_key else {
        return Err("Trusted public key is required for signed signaling.".into());
    };
    validate_trusted_public_key(peer_id, trusted_public_key)
}

fn validate_trusted_public_key(
    peer_id: &str,
    trusted_public_key: String,
) -> Result<String, String> {
    let trusted_public_key = trusted_public_key.trim().to_string();
    if trusted_public_key.is_empty() {
        return Err("Trusted public key is required for signed signaling.".into());
    }

    TrustedPeerIdentity::new(peer_id, trusted_public_key.clone())
        .validate()
        .map_err(|err| format!("Invalid trusted public key for {}: {}", peer_id, err))?;

    Ok(trusted_public_key)
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
        networking::signaling::auth::LocalPeerIdentity,
    };

    fn valid_public_key() -> String {
        LocalPeerIdentity::generate().public_key_base64()
    }

    #[test]
    fn contact_update_uses_current_contact_snapshot() {
        let mut state = state();
        state.services.database = ServicePhase::Ready;
        state.contacts.contacts = Arc::new(vec![Contact {
            id: 7,
            peer_id: "peer-a".into(),
            name: "Peer A".into(),
            address: None,
            trusted_public_key: Some("trusted-key".into()),
        }]);

        let commands = execute_contacts_action(
            &mut state,
            ContactsAction::UpdateAddressRequested { id: 7, new_address: "127.0.0.1:9000".into() },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::UpdateContact { id, address, .. }
            if *id == 7 && address.as_deref() == Some("127.0.0.1:9000")
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
                trusted_public_key: None,
            },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::Notify { level: NotificationLevel::Error, .. }
        )));
    }

    #[test]
    fn saving_contact_requires_valid_trusted_public_key() {
        let mut state = state();
        state.services.database = ServicePhase::Ready;

        let commands = execute_contacts_action(
            &mut state,
            ContactsAction::SaveRequested {
                peer_id: "peer-a".into(),
                name: "Peer A".into(),
                address: None,
                trusted_public_key: Some("not-a-key".into()),
            },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::Notify { level: NotificationLevel::Error, .. }
        )));
        assert!(!commands.iter().any(|command| matches!(command, AppCommand::SaveContact { .. })));
    }

    #[test]
    fn trusted_key_update_uses_current_contact_snapshot() {
        let mut state = state();
        state.services.database = ServicePhase::Ready;
        state.contacts.contacts = Arc::new(vec![Contact {
            id: 7,
            peer_id: "peer-a".into(),
            name: "Peer A".into(),
            address: Some("127.0.0.1:9000".into()),
            trusted_public_key: Some(valid_public_key()),
        }]);
        let new_key = valid_public_key();

        let commands = execute_contacts_action(
            &mut state,
            ContactsAction::UpdateTrustedPublicKeyRequested {
                id: 7,
                trusted_public_key: new_key.clone(),
            },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::UpdateContact {
                id,
                peer_id,
                address,
                trusted_public_key,
                ..
            } if *id == 7
                && peer_id == "peer-a"
                && address.as_deref() == Some("127.0.0.1:9000")
                && trusted_public_key.as_deref() == Some(new_key.as_str())
        )));
    }
}
