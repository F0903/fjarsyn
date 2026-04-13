use fjarsyn_core::config::{
    Config, clamp_max_depacket_latency, parse_max_depacket_latency_input,
    parse_target_bitrate_input,
};

use super::{SettingsMessage, SettingsScreen};
use crate::ui::shell::ShellContext;

#[derive(Debug, Clone)]
pub(crate) enum SettingsEffect {
    NotifyError(String),
    SaveConfig(Config),
}

// The settings workflow keeps UI field mutations local and emits only the work
// that needs runtime access, such as persistence or capture reconfiguration.
pub(crate) fn execute_settings_message(
    screen: &mut SettingsScreen,
    ctx: ShellContext<'_>,
    message: SettingsMessage,
) -> Vec<SettingsEffect> {
    match message {
        SettingsMessage::TabChanged(tab) => {
            screen.active_tab = tab;
            Vec::new()
        }
        SettingsMessage::TranscodingTypeChanged(value) => {
            screen.working_config.video.transcoding_type = value;
            Vec::new()
        }
        SettingsMessage::TargetResolutionChanged(value) => {
            screen.working_config.video.target_resolution = value;
            Vec::new()
        }
        SettingsMessage::TargetFramerateChanged(value) => {
            screen.working_config.video.target_framerate = value;
            Vec::new()
        }
        SettingsMessage::TargetBitrateChanged(value) => {
            screen.working_config.video.target_bitrate = value;
            Vec::new()
        }
        SettingsMessage::TargetBitrateInputChanged(value) => {
            parse_bitrate(screen, value).into_iter().collect()
        }
        SettingsMessage::RecordCursorChanged(value) => {
            screen.working_config.capture.record_cursor = value;
            Vec::new()
        }
        SettingsMessage::RecordingBorderIndicatorChanged(value) => {
            screen.working_config.capture.recording_border_indicator = value;
            Vec::new()
        }
        SettingsMessage::EnableUiPreviewChanged(value) => {
            screen.working_config.capture.enable_ui_preview = value;
            Vec::new()
        }
        SettingsMessage::MaxDepacketLatencyChanged(value) => {
            screen.working_config.network.max_depacket_latency = clamp_max_depacket_latency(value);
            Vec::new()
        }
        SettingsMessage::MaxDepacketLatencyInputChanged(value) => {
            parse_max_depacket_latency(screen, value).into_iter().collect()
        }
        SettingsMessage::SaveSettings => {
            vec![SettingsEffect::SaveConfig(screen.working_config.clone())]
        }
        SettingsMessage::DiscardSettings => {
            screen.working_config = ctx.config.clone();
            Vec::new()
        }
    }
}

fn parse_bitrate(screen: &mut SettingsScreen, value: String) -> Option<SettingsEffect> {
    match parse_target_bitrate_input(&value) {
        Ok(target_bitrate) => {
            screen.working_config.video.target_bitrate = target_bitrate;
            None
        }
        Err(message) => Some(SettingsEffect::NotifyError(message)),
    }
}

fn parse_max_depacket_latency(
    screen: &mut SettingsScreen,
    value: String,
) -> Option<SettingsEffect> {
    match parse_max_depacket_latency_input(&value) {
        Ok(latency) => {
            screen.working_config.network.max_depacket_latency = latency;
            None
        }
        Err(message) => Some(SettingsEffect::NotifyError(message)),
    }
}
