use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, pick_list, row, text, text_input},
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Framerate(CaptureFramerate),
    TranscodingType(FFmpegTranscodeType),
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
        let Some(config) = &mut self.pending_config else {
            return Task::none();
        };

        match message {
            Message::Settings(msg) => match msg {
                SettingsMessage::ConfigUpdate(field, value) => {
                    tracing::info!("Config update: {:?} {:?}", field, value);
                    match (field, value) {
                        (ConfigField::ServerUrl, ConfigValue::String(s)) => {
                            config.server_url = s;
                        }

                        (ConfigField::Framerate, ConfigValue::Framerate(rate)) => {
                            config.framerate = rate;
                        }

                        (ConfigField::TranscodingType, ConfigValue::TranscodingType(t)) => {
                            config.transcoding_type = t;
                        }

                        (ConfigField::Bitrate, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                config.bitrate = num;
                            } else {
                                tracing::error!("Unable to parse bitrate: {}", s);
                                //TODO: show field as invalid
                            }
                        }

                        (ConfigField::MaxDepacketLatency, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse() {
                                config.max_depacket_latency = num;
                            } else {
                                tracing::error!("Unable to parse max depacket latency: {}", s);
                                //TODO: show field as invalid
                            }
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

        let url_input = text_input("Signaling Server URL", &config.server_url)
            .on_input(|val| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::ServerUrl,
                    ConfigValue::String(val),
                ))
            })
            .padding(10);

        let framerate_pick = pick_list(CaptureFramerate::ALL, Some(config.framerate), |rate| {
            Message::Settings(SettingsMessage::ConfigUpdate(
                ConfigField::Framerate,
                ConfigValue::Framerate(rate),
            ))
        })
        .padding(10);

        let transcode_pick =
            pick_list(FFmpegTranscodeType::ALL, Some(config.transcoding_type), |t| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::TranscodingType,
                    ConfigValue::TranscodingType(t),
                ))
            })
            .padding(10);

        let bitrate_input = text_input("Bitrate (bps)", &config.bitrate.to_string())
            .on_input(|val| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::Bitrate,
                    ConfigValue::String(val),
                ))
            })
            .padding(10);

        let max_depacket_input =
            text_input("Max Depacket Latency (ms)", &config.max_depacket_latency.to_string())
                .on_input(|val| {
                    Message::Settings(SettingsMessage::ConfigUpdate(
                        ConfigField::MaxDepacketLatency,
                        ConfigValue::String(val),
                    ))
                })
                .padding(10);

        let save_button =
            button("Save").on_press(Message::Settings(SettingsMessage::SaveConfig)).padding(10);

        let back_button = button("Back").on_press(Message::Back).padding(10);

        let content = column![
            title,
            text("Server URL:"),
            url_input,
            text("Framerate:"),
            framerate_pick,
            text("Transcoding Type:"),
            transcode_pick,
            text("Bitrate:"),
            bitrate_input,
            text("Max Depacket Latency:"),
            max_depacket_input,
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
