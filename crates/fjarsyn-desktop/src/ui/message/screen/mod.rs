//! Screen-local UI intents owned by the application message boundary.

pub(in crate::ui) mod contacts;
pub(in crate::ui) mod settings;

#[derive(Debug, Clone)]
pub(in crate::ui) enum PeerMessage {
    DraftChanged(String),
    SendPressed,
    ToggleLocalPreview,
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Screen {
    Contacts(contacts::Message),
    Peer(PeerMessage),
    Settings(settings::Message),
}
