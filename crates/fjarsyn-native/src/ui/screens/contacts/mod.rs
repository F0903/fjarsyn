use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{AppContext, AppContextMut},
};

pub mod handlers;
mod view;
mod workflow;

#[derive(Debug, Clone)]
pub enum ContactsMessage {
    NameChanged(String),
    PeerIdChanged(String),
    AddressChanged(String),
    AddNewContact,
    ToggleAddForm,
}

#[derive(Debug, Clone)]
pub struct ContactsScreen {
    pub(crate) new_contact_name: String,
    pub(crate) new_contact_peer_id: String,
    pub(crate) new_contact_address: String,
    pub(crate) show_add_form: bool,
}

impl ContactsScreen {
    pub fn new(_ctx: AppContext<'_>) -> Self {
        Self {
            new_contact_name: String::new(),
            new_contact_peer_id: String::new(),
            new_contact_address: String::new(),
            show_add_form: false,
        }
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: AppContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: AppContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
