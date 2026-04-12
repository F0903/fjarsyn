use std::{fmt::Debug, sync::Arc};

use fjarsyn_core::{
    capture_providers::CaptureFramerate,
    config::Config,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
};
use iced::{Subscription, Task};

use crate::{
    settings_tab,
    ui::{
        app::{AppContext, AppContextMut},
        message::Message,
        screens::{Screen, settings::tabs::SettingsTab},
    },
};

mod components;
mod handlers;
mod tabs;
mod view;
mod workflow;

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    TabChanged(Arc<dyn SettingsTab>),
    TranscodingTypeChanged(FFmpegTranscodeType),
    TargetResolutionChanged(TargetResolution),
    TargetFramerateChanged(CaptureFramerate),
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

#[derive(Debug, Clone)]
pub struct SettingsScreen {
    working_config: Config,
    active_tab: Arc<dyn SettingsTab>,
}

impl SettingsScreen {
    pub fn new(config: &Config) -> Self {
        Self { working_config: config.clone(), active_tab: settings_tab!(tabs::capture::Capture) }
    }
}

impl Screen for SettingsScreen {
    fn subscription(&self, _ctx: AppContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: AppContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
