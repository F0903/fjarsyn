use crate::ui::screens::{
    call::CallMessage, contacts::ContactsMessage, home::HomeMessage, messages::MessagesMessage,
    settings::SettingsMessage,
};

#[derive(Debug, Clone)]
pub enum ScreenMessage {
    Home(HomeMessage),
    Messages(MessagesMessage),
    Contacts(ContactsMessage),
    Call(CallMessage),
    Settings(SettingsMessage),
}
