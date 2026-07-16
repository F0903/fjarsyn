use crate::ui::screens::{contacts::ContactsMessage, peer::PeerMessage, settings::SettingsMessage};

#[derive(Debug, Clone)]
pub enum ScreenMessage {
    Contacts(ContactsMessage),
    Peer(PeerMessage),
    Settings(SettingsMessage),
}
