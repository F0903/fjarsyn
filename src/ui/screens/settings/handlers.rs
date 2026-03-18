use iced::Task;

use super::{SettingsMessage, SettingsScreen};
use crate::ui::{
    app::AppState,
    message::{Message, ScreenMessage},
};

impl SettingsScreen {
    pub(crate) fn handle_message(&mut self, ctx: &mut AppState, message: Message) -> Task<Message> {
        let msg = match message {
            Message::Screen(ScreenMessage::Settings(s)) => s,
            _ => return Task::none(),
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
                    ctx.notify_error(format!("Invalid bitrate value: '{}'", val));
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

            SettingsMessage::EnableUiPreviewChanged(val) => {
                self.working_config.enable_ui_preview = val;
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
                    ctx.notify_error(format!("Unable to save settings: {}", e));
                }
                Task::none()
            }

            SettingsMessage::DiscardSettings => {
                self.working_config = ctx.config.clone();
                Task::none()
            }
        }
    }
}
