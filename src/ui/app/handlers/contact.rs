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
                let _ = service.refresh().await;
                Message::NoOp
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
                Task::none()
            }
        }
        ContactsServiceMessage::UpdateContactAddressConfirmed(id, addr) => {
            let Some(service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };
            let c = match service.contacts().iter().find(|c| c.id == id) {
                Some(c) => c.clone(),
                None => return Task::none(),
            };

            Task::future(async move {
                let res = service.update(id, c.peer_id, c.name, Some(addr)).await;
                match res {
                    Ok(_) => Message::ContactData(ContactsServiceMessage::LoadContacts),
                    Err(e) => Message::Notification(NotificationMessage::NotifyError(format!(
                        "Update Failed: {}",
                        e
                    ))),
                }
            })
        }
        ContactsServiceMessage::ContactSaved(res) => {
            if res.is_ok() {
                app.ctx.notify_success("Contact saved.");
                Task::done(Message::ContactData(ContactsServiceMessage::LoadContacts))
            } else {
                Task::none()
            }
        }
        ContactsServiceMessage::ContactDeleted(res) => {
            if res.is_ok() {
                app.ctx.notify_success("Contact deleted.");
                Task::done(Message::ContactData(ContactsServiceMessage::LoadContacts))
            } else {
                Task::none()
            }
        }
        ContactsServiceMessage::ContactsLoaded(_res) => Task::none(),
    }
}
