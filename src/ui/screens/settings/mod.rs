use std::{fmt::Debug, sync::Arc};

use iced::{Subscription, Task};

use crate::{
    capture_providers::CaptureFramerate,
    config::Config,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    settings_tab,
    ui::{
        app::AppContext,
        message::Message,
        screens::{Screen, settings::tabs::SettingsTab},
    },
};

mod components;
mod handlers;
mod tabs;
mod view;

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
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
