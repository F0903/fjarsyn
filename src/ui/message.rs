use std::sync::Arc;

use bytes::Bytes;

use crate::{
    networking::{
        discovery::DiscoveryEvent,
        webrtc::{WebRTC, WebRTCError, WebRTCEvent},
    },
    ui::screens::{
        call::CallMessage, contacts::ContactsMessage, home::HomeMessage, settings::SettingsMessage,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home,
    Contacts,
    Call,
    Settings,
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    PeerId(String),
    Address(String),
    ContactId(i64),
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,

    AcceptCall,
    DeclineCall,
    StartCall(CallTarget),

    // Screen messages
    Home(HomeMessage),
    Contacts(ContactsMessage),
    Call(CallMessage),
    Settings(SettingsMessage),

    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),
    WebRTCReinit,
    WebRTCReinitDone,
    WebRTCEvent(WebRTCEvent),
    DiscoveryEvent(DiscoveryEvent),
    PeerFound(crate::networking::discovery::PeerInfo),
    PeerRemoved(String), // By Peer ID
    PacketReceived(Bytes),

    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRawIdFetched((iced::window::Id, u64)),
    WindowMaximized(bool),
    SyncMaximized,

    NotifyError(String),
    NotifyInfo(String),
    NotifySuccess(String),

    CopyId(String),

    // Database / Contact messages
    DatabaseInitialized(Result<sqlx::SqlitePool, Arc<crate::Error>>),
    CaptureInitialized(
        Result<
            Arc<tokio::sync::RwLock<crate::capture_providers::PlatformCaptureProvider>>,
            Arc<crate::Error>,
        >,
    ),
    LoadContacts,
    ContactsLoaded(Result<Vec<crate::database::Contact>, Arc<crate::Error>>),
    SaveContact {
        peer_id: String,
        name: String,
        address: Option<String>,
    },
    DeleteContact(i64),
    ContactSaved(Result<i64, Arc<crate::Error>>),
    ContactDeleted(Result<(), Arc<crate::Error>>),
    UpdateContactAddress {
        id: i64,
        new_address: String,
    },
    UpdateContactAddressConfirmed(i64, String),

    Minimize,
    Maximize,
    Close,
    Drag,
    Resize(iced::window::Direction),

    Tick(std::time::Instant),
    DismissNotification(u64),

    Batch(Vec<Message>),

    NoOp,
}
