use super::{ContactsMessage, ContactsScreen};
use crate::ui::message::ContactsServiceMessage;

pub(crate) enum ContactsEffect {
    SaveContact { peer_id: String, name: String, address: Option<String> },
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

    if peer_id.is_empty() || name.is_empty() {
        return None;
    }

    screen.new_contact_name.clear();
    screen.new_contact_peer_id.clear();
    screen.new_contact_address.clear();
    screen.show_add_form = false;

    Some(ContactsEffect::SaveContact { peer_id, name, address })
}

pub(crate) fn into_message(effect: ContactsEffect) -> ContactsServiceMessage {
    match effect {
        ContactsEffect::SaveContact { peer_id, name, address } => {
            ContactsServiceMessage::SaveContact { peer_id, name, address }
        }
    }
}
