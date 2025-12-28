use iced::{
    Alignment, Element, Length, Padding, Subscription, Task,
    widget::{button, checkbox, column, container, pick_list, row, text, text_input},
};

use super::Screen;
use crate::{
    capture_providers::shared::CaptureFramerate,
    config::Config,
    media::ffmpeg::FFmpegTranscodeType,
    ui::{message::Message, state::AppContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Bitrate,
    Framerate,
    ServerUrl,
    MaxDepacketLatency,
    TranscodingType,
    RecordCursor,
    RecordingBorderIndicator,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
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

                        (ConfigField::Framerate, ConfigValue::Framerate(rate)) => {
                            pending.framerate = rate;
                        }

                        (ConfigField::TranscodingType, ConfigValue::TranscodingType(t)) => {
                            pending.transcoding_type = t;
                        }

                        (ConfigField::Bitrate, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                pending.bitrate = num;
                            } else {
                                tracing::error!("Unable to parse bitrate: {}", s);
                                //TODO: show field as invalid
                            }
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

        let framerate_pick = container(column![
            container(text("Capture Framerate:")).padding(Padding::ZERO.bottom(10)),
            pick_list(CaptureFramerate::ALL, Some(config.framerate), |rate| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::Framerate,
                    ConfigValue::Framerate(rate),
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

        let bitrate_input = container(column![
            container(text("Bitrate (bps):")).padding(Padding::ZERO.bottom(10)),
            text_input("", &config.bitrate.to_string())
                .on_input(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::Bitrate,
                        ConfigValue::String(val),
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
            framerate_pick,
            transcode_pick,
            bitrate_input,
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
