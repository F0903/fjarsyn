use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, pick_list, row, text, text_input},
};

use super::Screen;
use crate::{
    capture_providers::shared::CaptureFramerate,
    config::Config,
    ui::{message::Message, state::AppContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Bitrate,
    Framerate,
    ServerUrl,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Number(u32),
    Framerate(CaptureFramerate),
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
                        (ConfigField::Bitrate, ConfigValue::String(s)) => {
                            if let Ok(num) = s.parse::<u32>() {
                                config.bitrate = num;
                            }
                        }
                        (ConfigField::Bitrate, ConfigValue::Number(n)) => {
                            config.bitrate = n;
                        }

                        (ConfigField::Framerate, ConfigValue::Framerate(rate)) => {
                            config.framerate = rate;
                        }

                        (ConfigField::ServerUrl, ConfigValue::String(s)) => {
                            config.server_url = s;
                        }
                        _ => {}
                    }
                    Task::none()
                }

                SettingsMessage::SaveConfig => {
                    if let Some(pending) = self.pending_config.take() {
                        ctx.config = pending;
                        if let Err(e) = ctx.config.save() {
                            tracing::error!("Failed to save config: {}", e);
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

        let bitrate_input = text_input("Bitrate (bps)", &config.bitrate.to_string())
            .on_input(|val| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::Bitrate,
                    ConfigValue::String(val),
                ))
            })
            .padding(10);

        let framerate_pick =
            pick_list(&CaptureFramerate::ALL[..], Some(config.framerate), |rate| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::Framerate,
                    ConfigValue::Framerate(rate),
                ))
            })
            .padding(10);

        let url_input = text_input("Signaling Server URL", &config.server_url)
            .on_input(|val| {
                Message::Settings(SettingsMessage::ConfigUpdate(
                    ConfigField::ServerUrl,
                    ConfigValue::String(val),
                ))
            })
            .padding(10);

        let save_button =
            button("Save").on_press(Message::Settings(SettingsMessage::SaveConfig)).padding(10);

        let back_button =
            button("Back").on_press(Message::Navigate(crate::ui::message::Route::Home)).padding(10);

        let content = column![
            title,
            text("Bitrate:"),
            bitrate_input,
            text("Framerate:"),
            framerate_pick,
            text("Server URL:"),
            url_input,
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
