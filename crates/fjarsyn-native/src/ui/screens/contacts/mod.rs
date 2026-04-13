use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{ShellContext, ShellContextMut},
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
    pub fn new(_ctx: ShellContext<'_>) -> Self {
        Self {
            new_contact_name: String::new(),
            new_contact_peer_id: String::new(),
            new_contact_address: String::new(),
            show_add_form: false,
        }
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: ShellContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut ShellContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: ShellContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
