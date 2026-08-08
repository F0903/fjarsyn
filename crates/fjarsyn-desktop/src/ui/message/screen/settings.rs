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
    TargetBitrateChanged(u32),
    TargetBitrateInputChanged(String),
    RecordCursorChanged(bool),
    RecordingBorderIndicatorChanged(bool),
    EnableUiPreviewChanged(bool),
    MaxDepacketLatencyChanged(u16),
    MaxDepacketLatencyInputChanged(String),
    SaveSettings,
    DiscardSettings,
}
