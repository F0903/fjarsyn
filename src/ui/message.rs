use std::sync::Arc;

use bytes::Bytes;

use crate::{
    capture_providers::{
        PlatformCaptureItem,
        shared::{CaptureFramerate, Frame, Vector2},
    },
    networking::webrtc::{WebRTC, WebRTCError, WebRTCEvent},
    ui::screens::settings::{ConfigField, ConfigValue},
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

    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(PlatformCaptureItem),
    TryStopCapture,

    PlatformUserPickedCaptureItem(Result<PlatformCaptureItem, String>),

    FrameCaptured(Arc<Frame>),
    RemoteFrameReceived(Bytes),
    FrameRateSelected(CaptureFramerate),

    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),
    WebRTCEvent(WebRTCEvent),
    StartCall(String),
    DecodedFrameReady(Bytes, Vector2<i32>),

    CopyId(String),

    TargetIdChanged(String),

    WindowOpened(iced::window::Id),
    WindowIdFetched(u64),

    ConfigUpdate(ConfigField, ConfigValue),
    SaveConfig,

    Onboarding(super::screens::onboarding::OnboardingMessage),

    Error(String),
    NoOp,
}
