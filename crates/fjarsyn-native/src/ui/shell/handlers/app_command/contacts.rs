use iced::Task;

use crate::ui::{
    message::{ContactsServiceMessage, Message},
    shell::Fjarsyn,
};

pub(super) fn run_load_contacts(app: &mut Fjarsyn) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        Message::ContactData(ContactsServiceMessage::ContactsLoaded(
            match service.refresh().await {
                Ok(()) => Ok(service.contacts()),
                Err(err) => Err(err),
            },
        ))
    })
}

pub(super) fn run_save_contact(
    app: &mut Fjarsyn,
    peer_id: String,
    name: String,
    address: Option<String>,
    trusted_public_key: Option<String>,
) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        let result = match service.create(peer_id, name, address, trusted_public_key).await {
            Ok(_) => Ok(service.contacts()),
            Err(err) => Err(err),
        };
        Message::ContactData(ContactsServiceMessage::ContactSaved(result))
    })
}

pub(super) fn run_delete_contact(app: &mut Fjarsyn, id: i64) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        let result = match service.delete(id).await {
            Ok(()) => Ok(service.contacts()),
            Err(err) => Err(err),
        };
        Message::ContactData(ContactsServiceMessage::ContactDeleted(result))
    })
}

pub(super) fn run_update_contact(
    app: &mut Fjarsyn,
    id: i64,
    peer_id: String,
    name: String,
    address: Option<String>,
    trusted_public_key: Option<String>,
) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        let result = match service.update(id, peer_id, name, address, trusted_public_key).await {
            Ok(()) => Ok(service.contacts()),
            Err(err) => Err(err),
        };
        Message::ContactData(ContactsServiceMessage::ContactUpdated(result))
    })
}
