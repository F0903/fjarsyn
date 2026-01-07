use iced::{
    Alignment, Element, Length, Padding, Subscription, Task,
    widget::{button, checkbox, column, container, pick_list, row, text, text_input},
};

use super::Screen;
use crate::{
    capture_providers::CaptureFramerate,
    config::Config,
    define_enum_with_all,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    ui::{message::Message, state::AppContext},
    utils::vector2::Vector2,
};

define_enum_with_all! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ResolutionChoice {
        Source,
        Scale4K,
        Scale1080p,
        Scale720p,
        Scale480p,
        Scale360p,
    }
}

impl ResolutionChoice {
    fn from_target(target_res: TargetResolution) -> Self {
        match target_res {
            TargetResolution::Source => ResolutionChoice::Source,
            TargetResolution::Scale(size) => match (size.x, size.y) {
                (3840, 2160) => ResolutionChoice::Scale4K,
                (1920, 1080) => ResolutionChoice::Scale1080p,
                (1280, 720) => ResolutionChoice::Scale720p,
                (640, 480) => ResolutionChoice::Scale480p,
                (320, 240) => ResolutionChoice::Scale360p,
                _ => {
                    tracing::error!(
                        "Unsupported resolution specified: {:?}. Falling back to source",
                        size
                    );
                    ResolutionChoice::Source
                }
            },
        }
    }
}

impl Into<TargetResolution> for ResolutionChoice {
    fn into(self) -> TargetResolution {
        match self {
            ResolutionChoice::Source => TargetResolution::Source,
            ResolutionChoice::Scale4K => TargetResolution::Scale(Vector2::new(3840, 2160)),
            ResolutionChoice::Scale1080p => TargetResolution::Scale(Vector2::new(1920, 1080)),
            ResolutionChoice::Scale720p => TargetResolution::Scale(Vector2::new(1280, 720)),
            ResolutionChoice::Scale480p => TargetResolution::Scale(Vector2::new(640, 480)),
            ResolutionChoice::Scale360p => TargetResolution::Scale(Vector2::new(320, 240)),
        }
    }
}

impl std::fmt::Display for ResolutionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolutionChoice::Source => write!(f, "Source"),
            ResolutionChoice::Scale4K => write!(f, "4K"),
            ResolutionChoice::Scale1080p => write!(f, "1080p"),
            ResolutionChoice::Scale720p => write!(f, "720p"),
            ResolutionChoice::Scale480p => write!(f, "480p"),
            ResolutionChoice::Scale360p => write!(f, "360p"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Resolution,
    Framerate,
    Bitrate,
    ServerUrl,
    MaxDepacketLatency,
    TranscodingType,
    RecordCursor,
    RecordingBorderIndicator,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Resolution(ResolutionChoice),
    Framerate(CaptureFramerate),
    TranscodingType(FFmpegTranscodeType),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    ConfigUpdate(ConfigField, ConfigValue),
    SaveConfig,
}

#[derive(Debug, Clone)]
pub struct SettingsScreen {
    pub pending_config: Option<Config>,
}

impl SettingsScreen {
    pub fn new(current_config: Config) -> Self {
        Self { pending_config: Some(current_config) }
    }
}

impl Screen for SettingsScreen {
    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        // We operate on pending_config
        let Some(pending) = &mut self.pending_config else {
            return Task::none();
        };

        match message {
            Message::Settings(msg) => match msg {
                SettingsMessage::ConfigUpdate(field, value) => {
                    tracing::info!("Pending config update: {:?} {:?}", field, value);
                    match (field, value) {
                        (ConfigField::ServerUrl, ConfigValue::String(s)) => {
                            pending.server_url = s;
                        }

                        (ConfigField::Resolution, ConfigValue::Resolution(res)) => {
                            pending.target_resolution = res.into();
                        }

                        (ConfigField::Framerate, ConfigValue::Framerate(rate)) => {
                            pending.target_framerate = rate;
                        }

                        (ConfigField::Bitrate, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                pending.target_bitrate = num;
                            } else {
                                tracing::error!("Unable to parse bitrate: {}", s);
                                //TODO: show field as invalid
                            }
                        }

                        (ConfigField::TranscodingType, ConfigValue::TranscodingType(t)) => {
                            pending.transcoding_type = t;
                        }

                        (ConfigField::MaxDepacketLatency, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                pending.max_depacket_latency = num;
                            } else {
                                tracing::error!("Unable to parse max depacket latency: {}", s);
                                //TODO: show field as invalid
                            }
                        }

                        (ConfigField::RecordCursor, ConfigValue::Bool(b)) => {
                            pending.record_cursor = b;
                        }

                        (ConfigField::RecordingBorderIndicator, ConfigValue::Bool(b)) => {
                            pending.recording_border_indicator = b;
                        }

                        _ => {}
                    }

                    Task::none()
                }

                SettingsMessage::SaveConfig => {
                    if let Some(pending) = self.pending_config.take() {
                        ctx.config = pending;
                        if let Err(e) = ctx.config.save() {
                            let msg = format!("Failed to save config: {}", e);
                            tracing::error!(msg);
                            ctx.notifications.error(msg);
                        } else {
                            ctx.notifications.success("Config saved!");
                        }
                    }
                    Task::none()
                }
            },

            _ => Task::none(),
        }
    }

    fn view(&self, ctx: &AppContext) -> Element<'_, Message> {
        let config = self.pending_config.as_ref().unwrap_or(&ctx.config);

        let title = text("Settings").size(30);

        let url_input = container(column![
            container(text("Server URL:")).padding(Padding::ZERO.bottom(10)),
            text_input("", &config.server_url)
                .on_input(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::ServerUrl,
                        ConfigValue::String(val),
                    ))
                })
                .padding(10)
        ]);

        let resolution_pick = container(column![
            container(text("Target Resolution:")).padding(Padding::ZERO.bottom(10)),
            pick_list(
                ResolutionChoice::ALL,
                Some(ResolutionChoice::from_target(config.target_resolution)),
                |res| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::Resolution,
                        ConfigValue::Resolution(res),
                    ))
                }
            )
            .padding(10)
        ]);

        let framerate_pick = container(column![
            container(text("Target Framerate:")).padding(Padding::ZERO.bottom(10)),
            pick_list(CaptureFramerate::ALL, Some(config.target_framerate), |rate| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::Framerate,
                    ConfigValue::Framerate(rate),
                ))
            })
            .padding(10)
        ]);

        let bitrate_input = container(column![
            container(text("Target Bitrate (bps):")).padding(Padding::ZERO.bottom(10)),
            text_input("", &config.target_bitrate.to_string())
                .on_input(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::Bitrate,
                        ConfigValue::String(val),
                    ))
                })
                .padding(10)
        ]);

        let transcode_pick = container(column![
            container(text("Transcoding Type:")).padding(Padding::ZERO.bottom(10)),
            pick_list(FFmpegTranscodeType::ALL, Some(config.transcoding_type), |t| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::TranscodingType,
                    ConfigValue::TranscodingType(t),
                ))
            })
            .padding(10)
        ]);

        let max_depacket_input = container(column![
            container(text("Max Depacket Latency (ms)")).padding(Padding::ZERO.bottom(10)),
            text_input("", &config.max_depacket_latency.to_string())
                .on_input(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::MaxDepacketLatency,
                        ConfigValue::String(val),
                    ))
                })
                .padding(10)
        ]);

        let record_cursor_input = container(
            row![
                container(text("Record Cursor")),
                container(checkbox(config.record_cursor).on_toggle(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::RecordCursor,
                        ConfigValue::Bool(val),
                    ))
                }))
                .padding(10),
            ]
            .align_y(Alignment::Center),
        );

        let recording_border_indicator_input = container(
            row![
                container(text("Recording Border Indicator")),
                container(checkbox(config.recording_border_indicator).on_toggle(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::RecordingBorderIndicator,
                        ConfigValue::Bool(val),
                    ))
                }))
                .padding(10)
            ]
            .align_y(Alignment::Center),
        );

        let save_button =
            button("Save").on_press(Message::Settings(SettingsMessage::SaveConfig)).padding(10);

        let back_button = button("Back").on_press(Message::Back).padding(10);

        let content = column![
            title,
            url_input,
            resolution_pick,
            framerate_pick,
            bitrate_input,
            transcode_pick,
            max_depacket_input,
            record_cursor_input,
            recording_border_indicator_input,
            row![save_button, back_button].spacing(20)
        ]
        .spacing(20)
        .padding(20)
        .max_width(600);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }
}
