use std::sync::Arc;

use bytes::Bytes;

use crate::{
    networking::webrtc::{WebRTC, WebRTCError, WebRTCEvent},
    ui::screens::{
        capture::CaptureMessage, home::HomeMessage, onboarding::OnboardingMessage,
        settings::SettingsMessage,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home,
    Capture,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),

    // Sub-screen messages
    Home(HomeMessage),
    Capture(CaptureMessage),
    Settings(SettingsMessage),
    Onboarding(OnboardingMessage),

    // Global / Shared
    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),
    WebRTCEvent(WebRTCEvent),
    RemoteFrameReceived(Bytes),

    WindowOpened(iced::window::Id),
    WindowIdFetched(u64),

    Tick(std::time::Instant),

    Error(String),
    NoOp,
}
