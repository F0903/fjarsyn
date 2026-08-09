//! Settings-screen intents and stable tab identities.

use fjarsyn_engine::media::{
    codec::TranscodeType,
    video::{Framerate, TargetResolution},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum TabId {
    Capture,
    Network,
    Transcoding,
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    TabChanged(TabId),
    TranscodingTypeChanged(TranscodeType),
    TargetResolutionChanged(TargetResolution),
    TargetFramerateChanged(Framerate),
    TargetBitrateKbpsChanged(u32),
    TargetBitrateKbpsInputChanged(String),
    RecordCursorChanged(bool),
    RecordingBorderIndicatorChanged(bool),
    EnableUiPreviewChanged(bool),
    MaxDepacketLatencyMsChanged(u16),
    MaxDepacketLatencyMsInputChanged(String),
    SaveSettings,
    SaveAndRetryStartup,
    DiscardSettings,
}
