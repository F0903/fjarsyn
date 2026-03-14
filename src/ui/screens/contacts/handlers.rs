use iced::Task;

use super::{ContactsMessage, ContactsScreen};
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

impl ContactsScreen {
    pub(crate) fn handle_message(
        &mut self,
        _ctx: &mut AppContext,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::Contacts(msg) => match msg {
                ContactsMessage::NameChanged(val) => {
                    self.new_contact_name = val;
                    Task::none()
                }
                ContactsMessage::PeerIdChanged(val) => {
                    self.new_contact_peer_id = val;
                    Task::none()
                }
                ContactsMessage::AddressChanged(val) => {
                    self.new_contact_address = val;
                    Task::none()
                }
                ContactsMessage::ToggleAddForm => {
                    self.show_add_form = !self.show_add_form;
                    Task::none()
                }
                ContactsMessage::AddNewContact => {
                    let peer_id = self.new_contact_peer_id.trim().to_string();
                    let name = self.new_contact_name.trim().to_string();
                    let address = if self.new_contact_address.trim().is_empty() {
                        None
                    } else {
                        Some(self.new_contact_address.trim().to_string())
                    };

                    if peer_id.is_empty() || name.is_empty() {
                        return Task::none();
                    }

                    self.new_contact_name.clear();
                    self.new_contact_peer_id.clear();
                    self.new_contact_address.clear();
                    self.show_add_form = false;

                    Task::done(Message::SaveContact { peer_id, name, address })
                }
            },
            _ => Task::none(),
        }
    }
}
