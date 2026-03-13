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
        let msg = match message {
            Message::Settings(s) => s,
            _ => return Task::none(), // Only handle Settings messages
        };
        match msg {
            SettingsMessage::TabChanged(tab) => {
                self.active_tab = tab;
                Task::none()
            }

            SettingsMessage::TranscodingTypeChanged(val) => {
                self.working_config.transcoding_type = val;
                Task::none()
            }

            SettingsMessage::TargetResolutionChanged(val) => {
                self.working_config.target_resolution = val;
                Task::none()
            }

            SettingsMessage::TargetFramerateChanged(val) => {
                self.working_config.target_framerate = val;
                Task::none()
            }

            SettingsMessage::TargetBitrateChanged(val) => {
                self.working_config.target_bitrate = val;
                Task::none()
            }

            SettingsMessage::TargetBitrateInputChanged(val) => {
                if let Ok(num) = val.parse::<u32>() {
                    self.working_config.target_bitrate = num * 1000;
                } else {
                    ctx.notify_error(format!("Invalid max depacket latency value: '{}'", val));
                }
                Task::none()
            }

            SettingsMessage::RecordCursorChanged(val) => {
                self.working_config.record_cursor = val;
                Task::none()
            }

            SettingsMessage::RecordingBorderIndicatorChanged(val) => {
                self.working_config.recording_border_indicator = val;
                Task::none()
            }

            SettingsMessage::MaxDepacketLatencyChanged(val) => {
                self.working_config.max_depacket_latency = val;
                Task::none()
            }

            SettingsMessage::MaxDepacketLatencyInputChanged(val) => {
                if let Ok(num) = val.parse::<u16>() {
                    self.working_config.max_depacket_latency = num;
                } else {
                    ctx.notify_error(format!("Invalid max depacket latency value: '{}'", val));
                }
                Task::none()
            }

            SettingsMessage::SaveSettings => {
                ctx.config = self.working_config.clone();
                if let Err(e) = ctx.config.save() {
                    ctx.notify_error(format!("Unable to save settings! '{}'", e));
                }
                Task::none()
            }

            SettingsMessage::DiscardSettings => {
                self.working_config = ctx.config.clone();
                Task::none()
            }
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
