use iced::{
    Element, Length, Subscription, Task,
    widget::{button, column, container, pick_list, row, text, text_input},
};

use super::Screen;
use crate::{
    capture_providers::shared::CaptureFramerate,
    ui::{message::Message, state::State},
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
pub struct SettingsScreen {}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {}
    }
}

impl Screen for SettingsScreen {
    fn update(&self, state: &mut State, message: Message) -> Task<Message> {
        // We operate on pending_config
        let Some(config) = &mut state.pending_config else {
            return Task::none();
        };

        match message {
            Message::ConfigUpdate(field, value) => {
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

                    _ => {
                        tracing::warn!(
                            "Type mismatch or unimplemented handler for config update: {:?} ",
                            field
                        );
                    }
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn view(&self, state: &State) -> Element<'_, Message> {
        let config = state.pending_config.as_ref().unwrap_or(&state.config);

        let title = text("Settings").size(30);

        let bitrate_input = text_input("Bitrate (bps)", &config.bitrate.to_string())
            .on_input(|val| Message::ConfigUpdate(ConfigField::Bitrate, ConfigValue::String(val)))
            .padding(10);

        let framerate_pick =
            pick_list(&CaptureFramerate::ALL[..], Some(config.framerate), |rate| {
                Message::ConfigUpdate(ConfigField::Framerate, ConfigValue::Framerate(rate))
            })
            .padding(10);

        let url_input = text_input("Signaling Server URL", &config.server_url)
            .on_input(|val| Message::ConfigUpdate(ConfigField::ServerUrl, ConfigValue::String(val)))
            .padding(10);

        let save_button = button("Save").on_press(Message::SaveConfig).padding(10);

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

    fn subscription(&self, _state: &State) -> Subscription<Message> {
        Subscription::none()
    }
}
