use std::sync::Arc;

use bytes::Bytes;

use crate::{
    networking::webrtc::{WebRTC, WebRTCError, WebRTCEvent},
    ui::screens::{
        call::CallMessage, home::HomeMessage, onboarding::OnboardingMessage,
        settings::SettingsMessage,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home,
    Call,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,

    // Sub-screen messages
    Home(HomeMessage),
    Call(CallMessage),
    Settings(SettingsMessage),
    Onboarding(OnboardingMessage),

    // Global / Shared
    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),
    WebRTCEvent(WebRTCEvent),
    PacketReceived(Bytes),

    WindowOpened(iced::window::Id),
    WindowIdFetched(u64),

    Tick(std::time::Instant),
    DismissNotification(u64),

    NoOp,
}
