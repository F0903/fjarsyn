use crate::ui::screens::{
    call::CallMessage, contacts::ContactsMessage, home::HomeMessage, settings::SettingsMessage,
};

#[derive(Debug, Clone)]
pub enum ScreenMessage {
    Home(HomeMessage),
    Contacts(ContactsMessage),
    Call(CallMessage),
    Settings(SettingsMessage),
}
