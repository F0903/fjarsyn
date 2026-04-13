use iced::Task;

use crate::ui::{
    app::Fjarsyn,
    message::{ContactsServiceMessage, Message},
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
) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        let result = match service.create(peer_id, name, address).await {
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

pub(super) fn run_update_contact_address(
    app: &mut Fjarsyn,
    id: i64,
    peer_id: String,
    name: String,
    address: String,
) -> Task<Message> {
    let Some(service) = app.runtime.services.contacts_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        let result = match service.update(id, peer_id, name, Some(address)).await {
            Ok(()) => Ok(service.contacts()),
            Err(err) => Err(err),
        };
        Message::ContactData(ContactsServiceMessage::ContactUpdated(result))
    })
}
