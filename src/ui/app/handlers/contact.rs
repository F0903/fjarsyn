use iced::Task;

use crate::ui::{
    app::{
        Fjarsyn,
        workflows::contact::{self, ContactEffect},
    },
    message::{ContactsServiceMessage, Message, NotificationMessage},
};

pub fn handle_contact_msg(app: &mut Fjarsyn, message: ContactsServiceMessage) -> Task<Message> {
    let effects = contact::reduce(app, message);
    run_effects(app, effects)
}

fn run_effects(app: &mut Fjarsyn, effects: Vec<ContactEffect>) -> Task<Message> {
    let mut tasks = Vec::with_capacity(effects.len());
    for effect in effects {
        tasks.push(run_effect(app, effect));
    }
    Task::batch(tasks)
}

fn run_effect(app: &mut Fjarsyn, effect: ContactEffect) -> Task<Message> {
    match effect {
        ContactEffect::NotifyError(message) => {
            app.ctx.notify_error(message);
            Task::none()
        }
        ContactEffect::NotifyInfo(message) => {
            app.ctx.notify_info(message);
            Task::none()
        }
        ContactEffect::NotifySuccess(message) => {
            app.ctx.notify_success(message);
            Task::none()
        }
        ContactEffect::LoadContacts(service) => Task::future(async move {
            match service.refresh().await {
                Ok(_) => Message::NoOp,
                Err(err) => Message::Notification(NotificationMessage::NotifyError(format!(
                    "Unable to load contacts: {}",
                    err
                ))),
            }
        }),
        ContactEffect::SaveContact { service, peer_id, name, address } => {
            Task::future(async move {
                Message::ContactData(ContactsServiceMessage::ContactSaved(
                    service.create(peer_id, name, address).await,
                ))
            })
        }
        ContactEffect::DeleteContact { service, id } => Task::future(async move {
            Message::ContactData(ContactsServiceMessage::ContactDeleted(service.delete(id).await))
        }),
        ContactEffect::ConfirmUpdateContactAddress { id, new_address } => {
            Task::done(Message::ContactData(ContactsServiceMessage::UpdateContactAddressConfirmed(
                id,
                new_address,
            )))
        }
        ContactEffect::UpdateContactAddress { service, contact, new_address } => {
            let contact_name = contact.name.clone();

            Task::future(async move {
                let result = service
                    .update(contact.id, contact.peer_id, contact.name, Some(new_address))
                    .await;

                match result {
                    Ok(_) => Message::Notification(NotificationMessage::NotifySuccess(format!(
                        "Updated address for {}.",
                        contact_name
                    ))),
                    Err(err) => Message::Notification(NotificationMessage::NotifyError(format!(
                        "Update Failed: {}",
                        err
                    ))),
                }
            })
        }
    }
}
