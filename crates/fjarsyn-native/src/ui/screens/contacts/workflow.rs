use super::{ContactsMessage, ContactsScreen};
use crate::ui::message::ContactsServiceMessage;

pub(crate) enum ContactsEffect {
    SaveContact {
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    },
    UpdateTrustedPublicKey {
        id: i64,
        trusted_public_key: String,
    },
}

// Contacts UI state is mostly local form state. Emit an effect only when the
// user has produced a valid contact payload to persist.
pub(crate) fn execute_contacts_message(
    screen: &mut ContactsScreen,
    message: ContactsMessage,
) -> Vec<ContactsEffect> {
    match message {
        ContactsMessage::NameChanged(value) => {
            screen.new_contact_name = value;
            Vec::new()
        }
        ContactsMessage::PeerIdChanged(value) => {
            screen.new_contact_peer_id = value;
            Vec::new()
        }
        ContactsMessage::AddressChanged(value) => {
            screen.new_contact_address = value;
            Vec::new()
        }
        ContactsMessage::TrustedPublicKeyChanged(value) => {
            screen.new_contact_trusted_public_key = value;
            Vec::new()
        }
        ContactsMessage::StartTrustedPublicKeyEdit { id, current_key } => {
            screen.editing_contact_id = Some(id);
            screen.editing_trusted_public_key = current_key.unwrap_or_default();
            Vec::new()
        }
        ContactsMessage::ExistingTrustedPublicKeyChanged(value) => {
            screen.editing_trusted_public_key = value;
            Vec::new()
        }
        ContactsMessage::CancelTrustedPublicKeyEdit => {
            clear_trusted_public_key_edit(screen);
            Vec::new()
        }
        ContactsMessage::SaveTrustedPublicKeyEdit => {
            build_update_trusted_public_key_effect(screen).into_iter().collect()
        }
        ContactsMessage::ToggleAddForm => {
            screen.show_add_form = !screen.show_add_form;
            Vec::new()
        }
        ContactsMessage::AddNewContact => build_save_contact_effect(screen).into_iter().collect(),
    }
}

fn build_save_contact_effect(screen: &mut ContactsScreen) -> Option<ContactsEffect> {
    let peer_id = screen.new_contact_peer_id.trim().to_string();
    let name = screen.new_contact_name.trim().to_string();
    let address = if screen.new_contact_address.trim().is_empty() {
        None
    } else {
        Some(screen.new_contact_address.trim().to_string())
    };
    let trusted_public_key = if screen.new_contact_trusted_public_key.trim().is_empty() {
        None
    } else {
        Some(screen.new_contact_trusted_public_key.trim().to_string())
    };

    if peer_id.is_empty() || name.is_empty() || trusted_public_key.is_none() {
        return None;
    }

    screen.new_contact_name.clear();
    screen.new_contact_peer_id.clear();
    screen.new_contact_address.clear();
    screen.new_contact_trusted_public_key.clear();
    screen.show_add_form = false;

    Some(ContactsEffect::SaveContact { peer_id, name, address, trusted_public_key })
}

fn build_update_trusted_public_key_effect(screen: &mut ContactsScreen) -> Option<ContactsEffect> {
    let id = screen.editing_contact_id?;
    let trusted_public_key = screen.editing_trusted_public_key.trim().to_string();
    clear_trusted_public_key_edit(screen);
    Some(ContactsEffect::UpdateTrustedPublicKey { id, trusted_public_key })
}

fn clear_trusted_public_key_edit(screen: &mut ContactsScreen) {
    screen.editing_contact_id = None;
    screen.editing_trusted_public_key.clear();
}

pub(crate) fn into_message(effect: ContactsEffect) -> ContactsServiceMessage {
    match effect {
        ContactsEffect::SaveContact { peer_id, name, address, trusted_public_key } => {
            ContactsServiceMessage::SaveContact { peer_id, name, address, trusted_public_key }
        }
        ContactsEffect::UpdateTrustedPublicKey { id, trusted_public_key } => {
            ContactsServiceMessage::UpdateContactTrustedPublicKey { id, trusted_public_key }
        }
    }
}
