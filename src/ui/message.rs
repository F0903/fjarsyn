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
pub enum Message {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,

    AcceptCall,
    DeclineCall,

    // Sub-screen messages
    Home(HomeMessage),
    Contacts(ContactsMessage),
    Call(CallMessage),
    Settings(SettingsMessage),

    // Global / Shared
    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),
    WebRTCReinit,
    WebRTCReinitDone,
    WebRTCEvent(WebRTCEvent),
    DiscoveryEvent(DiscoveryEvent),
    PacketReceived(Bytes),

    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRawIdFetched((iced::window::Id, u64)),

    Tick(std::time::Instant),
    DismissNotification(u64),

    NoOp,
}
