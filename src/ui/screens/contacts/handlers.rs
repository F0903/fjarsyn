use iced::Task;

use crate::{
    services::contact_service::ContactService,
    ui::{app::AppContext, message::Message},
};

pub fn handle_load_contacts(ctx: &AppContext) -> Task<Message> {
    let Some(db) = ctx.db.clone() else {
        return Task::none();
    };
    Task::future(async move { Message::ContactsLoaded(ContactService::list(&db).await) })
}

pub fn handle_save_contact(
    ctx: &AppContext,
    peer_id: String,
    name: String,
    address: Option<String>,
) -> Task<Message> {
    let Some(db) = ctx.db.clone() else {
        return Task::none();
    };
    Task::future(async move {
        Message::ContactSaved(ContactService::create(&db, peer_id, name, address).await)
    })
}

pub fn handle_delete_contact(ctx: &AppContext, id: i64) -> Task<Message> {
    let Some(db) = ctx.db.clone() else {
        return Task::none();
    };
    Task::future(async move { Message::ContactDeleted(ContactService::delete(&db, id).await) })
}

pub fn handle_update_address(ctx: &mut AppContext, id: i64, addr: String) -> Task<Message> {
    if let Some(c) = ctx.contacts.iter().find(|c| c.id == id) {
        ctx.notify_info(format!("Updating address for {}...", c.name));
        return Task::done(Message::UpdateContactAddressConfirmed(id, addr));
    }
    Task::none()
}

pub fn handle_update_confirmed(ctx: &AppContext, id: i64, addr: String) -> Task<Message> {
    let Some(db) = ctx.db.clone() else {
        return Task::none();
    };
    let c = match ctx.contacts.iter().find(|c| c.id == id) {
        Some(c) => c.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        let res = ContactService::update(&db, id, c.peer_id, c.name, Some(addr)).await;
        match res {
            Ok(_) => Message::LoadContacts,
            Err(e) => Message::NotifyError(format!("Update Failed: {}", e)),
        }
    })
}
