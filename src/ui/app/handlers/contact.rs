use iced::Task;

use crate::ui::{
    app::Fjarsyn,
    message::{ContactsServiceMessage, Message, NotificationMessage},
};

pub fn handle_contact_msg(app: &mut Fjarsyn, msg: ContactsServiceMessage) -> Task<Message> {
    match msg {
        ContactsServiceMessage::LoadContacts => {
            let Some(service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                match service.refresh().await {
                    Ok(_) => Message::NoOp,
                    Err(err) => Message::Notification(NotificationMessage::NotifyError(format!(
                        "Unable to load contacts: {}",
                        err
                    ))),
                }
            })
        }
        ContactsServiceMessage::SaveContact { peer_id, name, address } => {
            let Some(service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };
            let peer_id = peer_id.clone();
            let name = name.clone();
            let address = address.clone();

            Task::future(async move {
                Message::ContactData(ContactsServiceMessage::ContactSaved(
                    service.create(peer_id, name, address).await,
                ))
            })
        }
        ContactsServiceMessage::DeleteContact(id) => {
            let Some(service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                Message::ContactData(ContactsServiceMessage::ContactDeleted(
                    service.delete(id).await,
                ))
            })
        }
        ContactsServiceMessage::UpdateContactAddress { id, new_address } => {
            let Some(service) = app.ctx.services.contacts_service.as_ref() else {
                return Task::none();
            };
            if let Some(c) = service.contacts().iter().find(|c| c.id == id) {
                app.ctx.notify_info(format!("Updating address for {}...", c.name));
                Task::done(Message::ContactData(
                    ContactsServiceMessage::UpdateContactAddressConfirmed(id, new_address.clone()),
                ))
            } else {
                Task::done(Message::Notification(NotificationMessage::NotifyError(
                    "Contact not found.".into(),
                )))
            }
        }
        ContactsServiceMessage::UpdateContactAddressConfirmed(id, addr) => {
            let Some(service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };
            let c = match service.contacts().iter().find(|c| c.id == id) {
                Some(c) => c.clone(),
                None => {
                    return Task::done(Message::Notification(NotificationMessage::NotifyError(
                        "Contact not found.".into(),
                    )));
                }
            };
            let contact_name = c.name.clone();

            Task::future(async move {
                let res = service.update(id, c.peer_id, c.name, Some(addr)).await;
                match res {
                    Ok(_) => Message::Notification(NotificationMessage::NotifySuccess(format!(
                        "Updated address for {}.",
                        contact_name
                    ))),
                    Err(e) => Message::Notification(NotificationMessage::NotifyError(format!(
                        "Update Failed: {}",
                        e
                    ))),
                }
            })
        }
        ContactsServiceMessage::ContactSaved(res) => match res {
            Ok(_) => {
                app.ctx.notify_success("Contact saved.");
                Task::none()
            }
            Err(err) => {
                app.ctx.notify_error(format!("Save Failed: {}", err));
                Task::none()
            }
        },
        ContactsServiceMessage::ContactDeleted(res) => match res {
            Ok(_) => {
                app.ctx.notify_success("Contact deleted.");
                Task::none()
            }
            Err(err) => {
                app.ctx.notify_error(format!("Delete Failed: {}", err));
                Task::none()
            }
        },
    }
}
