use iced::{Subscription, Task};

use crate::ui::{app::AppContext, message::Message, screens::Screen};

pub mod handlers;
mod view;

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
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self {
            new_contact_name: String::new(),
            new_contact_peer_id: String::new(),
            new_contact_address: String::new(),
            show_add_form: false,
        }
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut AppContext, message: Message) -> Task<Message> {
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

    fn view<'a>(&'a self, ctx: &'a AppContext) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
