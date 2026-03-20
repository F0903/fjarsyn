use std::sync::Arc;

use crate::{
    services::contacts_service::{Contact, ContactsService},
    ui::{app::Fjarsyn, message::ContactsServiceMessage},
};

pub(crate) enum ContactEffect {
    NotifyError(String),
    NotifyInfo(String),
    NotifySuccess(String),
    LoadContacts(Arc<ContactsService>),
    SaveContact {
        service: Arc<ContactsService>,
        peer_id: String,
        name: String,
        address: Option<String>,
    },
    DeleteContact {
        service: Arc<ContactsService>,
        id: i64,
    },
    ConfirmUpdateContactAddress {
        id: i64,
        new_address: String,
    },
    UpdateContactAddress {
        service: Arc<ContactsService>,
        contact: Contact,
        new_address: String,
    },
}

// Contact workflows stay Iced-free and describe only the state changes and
// async work that should happen next.
pub(crate) fn reduce(app: &mut Fjarsyn, message: ContactsServiceMessage) -> Vec<ContactEffect> {
    match message {
        ContactsServiceMessage::LoadContacts => app
            .ctx
            .services
            .contacts_service
            .clone()
            .map(ContactEffect::LoadContacts)
            .into_iter()
            .collect(),
        ContactsServiceMessage::SaveContact { peer_id, name, address } => app
            .ctx
            .services
            .contacts_service
            .clone()
            .map(|service| ContactEffect::SaveContact { service, peer_id, name, address })
            .into_iter()
            .collect(),
        ContactsServiceMessage::DeleteContact(id) => app
            .ctx
            .services
            .contacts_service
            .clone()
            .map(|service| ContactEffect::DeleteContact { service, id })
            .into_iter()
            .collect(),
        ContactsServiceMessage::UpdateContactAddress { id, new_address } => {
            reduce_update_contact_address(app, id, new_address)
        }
        ContactsServiceMessage::UpdateContactAddressConfirmed(id, new_address) => {
            reduce_confirmed_update(app, id, new_address)
        }
        ContactsServiceMessage::ContactSaved(result) => match result {
            Ok(_) => vec![ContactEffect::NotifySuccess("Contact saved.".into())],
            Err(err) => vec![ContactEffect::NotifyError(format!("Save Failed: {}", err))],
        },
        ContactsServiceMessage::ContactDeleted(result) => match result {
            Ok(_) => vec![ContactEffect::NotifySuccess("Contact deleted.".into())],
            Err(err) => vec![ContactEffect::NotifyError(format!("Delete Failed: {}", err))],
        },
    }
}

fn reduce_update_contact_address(
    app: &mut Fjarsyn,
    id: i64,
    new_address: String,
) -> Vec<ContactEffect> {
    let Some(service) = app.ctx.services.contacts_service.as_ref() else {
        return Vec::new();
    };

    match service.contacts().iter().find(|contact| contact.id == id) {
        Some(contact) => vec![
            ContactEffect::NotifyInfo(format!("Updating address for {}...", contact.name)),
            ContactEffect::ConfirmUpdateContactAddress { id, new_address },
        ],
        None => vec![ContactEffect::NotifyError("Contact not found.".into())],
    }
}

fn reduce_confirmed_update(app: &mut Fjarsyn, id: i64, new_address: String) -> Vec<ContactEffect> {
    let Some(service) = app.ctx.services.contacts_service.clone() else {
        return Vec::new();
    };

    match service.contacts().iter().find(|contact| contact.id == id).cloned() {
        Some(contact) => {
            vec![ContactEffect::UpdateContactAddress { service, contact, new_address }]
        }
        None => vec![ContactEffect::NotifyError("Contact not found.".into())],
    }
}
