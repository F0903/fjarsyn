use iced::{
    Alignment, Element, Length, Padding, Subscription, Task,
    widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::Screen;
use crate::{
    capture_providers::CaptureFramerate,
    config::Config,
    define_enum_with_all,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    ui::{fonts, message::Message, state::AppContext, theme},
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
    DiscardChanges,
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
        let Some(pending) = &mut self.pending_config else {
            return Task::none();
        };

        match message {
            Message::Settings(msg) => match msg {
                SettingsMessage::ConfigUpdate(field, value) => {
                    match (field, value) {
                        (ConfigField::Resolution, ConfigValue::Resolution(res)) => {
                            pending.target_resolution = res.into();
                        }
                        (ConfigField::Framerate, ConfigValue::Framerate(rate)) => {
                            pending.target_framerate = rate;
                        }
                        (ConfigField::Bitrate, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                pending.target_bitrate = num;
                            }
                        }
                        (ConfigField::TranscodingType, ConfigValue::TranscodingType(t)) => {
                            pending.transcoding_type = t;
                        }
                        (ConfigField::MaxDepacketLatency, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                pending.max_depacket_latency = num;
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
                            ctx.notifications.success("Settings saved successfully!");
                        }
                        // Re-initialize with updated config
                        self.pending_config = Some(ctx.config.clone());
                    }
                    Task::none()
                }

                SettingsMessage::DiscardChanges => {
                    self.pending_config = Some(ctx.config.clone());
                    ctx.notifications.info("Changes discarded.");
                    Task::none()
                }
            },
            _ => Task::none(),
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
        let config = self.pending_config.as_ref().unwrap_or(&ctx.config);
        let has_changes = self.pending_config.as_ref().map(|p| p != &ctx.config).unwrap_or(false);

        let header = column![
            text("Settings").size(32).style(text::primary).font(fonts::outfit::BOLD),
            text("Configure your streaming and capture preferences")
                .size(14)
                .style(text::secondary),
        ]
        .spacing(10);

        let stream_quality = self.section(
            "Stream Quality",
            lucide::video(),
            column![
                self.setting_row(
                    "Target Resolution",
                    "Choose the output resolution for the stream.",
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
                    .width(150)
                    .padding(8)
                ),
                self.setting_row(
                    "Target Framerate",
                    "Higher framerates provide smoother motion but require more bandwidth.",
                    pick_list(CaptureFramerate::ALL, Some(config.target_framerate), |rate| {
                        Message::Settings(SettingsMessage::ConfigUpdate(
                            ConfigField::Framerate,
                            ConfigValue::Framerate(rate),
                        ))
                    })
                    .width(150)
                    .padding(8)
                ),
                self.setting_row(
                    "Target Bitrate",
                    "Control the stream quality and bandwidth usage (bps).",
                    text_input("", &config.target_bitrate.to_string())
                        .on_input(|val| {
                            Message::Settings(SettingsMessage::ConfigUpdate(
                                ConfigField::Bitrate,
                                ConfigValue::String(val),
                            ))
                        })
                        .width(150)
                        .padding(8)
                        .style(theme::text_input_style)
                ),
                self.setting_row(
                    "Transcoding",
                    "Choose between hardware or software encoding.",
                    pick_list(FFmpegTranscodeType::ALL, Some(config.transcoding_type), |t| {
                        Message::Settings(SettingsMessage::ConfigUpdate(
                            ConfigField::TranscodingType,
                            ConfigValue::TranscodingType(t),
                        ))
                    })
                    .width(150)
                    .padding(8)
                ),
            ],
        );

        let networking = self.section(
            "Networking",
            lucide::network(),
            column![
                self.setting_row(
                    "Max Depacket Latency",
                    "Maximum buffer time for reordering incoming packets (ms).",
                    text_input("", &config.max_depacket_latency.to_string())
                        .on_input(|val| {
                            Message::Settings(SettingsMessage::ConfigUpdate(
                                ConfigField::MaxDepacketLatency,
                                ConfigValue::String(val),
                            ))
                        })
                        .width(150)
                        .padding(8)
                        .style(theme::text_input_style)
                )
            ],
        );

        let capture = self.section(
            "Capture Preferences",
            lucide::monitor(),
            column![
                self.setting_row(
                    "Record Cursor",
                    "Whether to include the mouse cursor in the screen share.",
                    checkbox(config.record_cursor).on_toggle(|val| {
                        Message::Settings(SettingsMessage::ConfigUpdate(
                            ConfigField::RecordCursor,
                            ConfigValue::Bool(val),
                        ))
                    })
                ),
                self.setting_row(
                    "Border Indicator",
                    "Show a yellow border around the recorded area.",
                    checkbox(config.recording_border_indicator).on_toggle(|val| {
                        Message::Settings(SettingsMessage::ConfigUpdate(
                            ConfigField::RecordingBorderIndicator,
                            ConfigValue::Bool(val),
                        ))
                    })
                ),
            ],
        );

        let footer = row![
            button(row![lucide::save().size(16), text("Save Changes")].spacing(10))
                .on_press_maybe(has_changes.then(|| Message::Settings(SettingsMessage::SaveConfig)))
                .padding(Padding::from([10, 20]))
                .style(|theme, status| theme::button_style(theme, status, true)),
            button("Discard Changes")
                .on_press_maybe(
                    has_changes.then(|| Message::Settings(SettingsMessage::DiscardChanges))
                )
                .padding(Padding::from([10, 20]))
                .style(|theme, status| theme::button_style(theme, status, false)),
        ]
        .spacing(15)
        .align_y(Alignment::Center);

        let content = column![header, stream_quality, networking, capture, footer]
            .spacing(30)
            .padding(20)
            .max_width(800);

        container(scrollable(content))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .into()
    }

    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }
}

impl SettingsScreen {
    fn section<'a>(
        &self,
        title: &'a str,
        icon: iced::widget::Text<'a>,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        container(
            column![
                row![
                    icon.size(20).style(text::primary),
                    text(title).size(20).font(fonts::outfit::BOLD)
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                container(content).padding(Padding::ZERO.top(10.0))
            ]
            .spacing(15),
        )
        .padding(20)
        .style(theme::section_container)
        .width(Length::Fill)
        .into()
    }

    fn setting_row<'a>(
        &self,
        label: &'a str,
        description: &'a str,
        control: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        row![
            column![text(label).size(16), text(description).size(12).style(text::secondary)]
                .spacing(5)
                .width(Length::Fill),
            container(control).width(Length::Shrink).align_y(Alignment::Center)
        ]
        .padding(Padding::from([10, 0]))
        .align_y(Alignment::Center)
        .into()
    }
}
