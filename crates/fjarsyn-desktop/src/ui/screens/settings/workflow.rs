use fjarsyn_engine::config::{
    Config, clamp_max_depacket_latency, parse_max_depacket_latency_input,
    parse_target_bitrate_input,
};

use super::{Screen, tabs};
use crate::ui::{message::screen::settings::Message, presentation::Context};

#[derive(Debug, Clone)]
pub(super) enum Effect {
    NotifyError(String),
    SaveConfig(Config),
}

// The settings workflow keeps UI field mutations local and emits only the work
// that needs runtime access, such as persistence or capture reconfiguration.
pub(super) fn execute_settings_message(
    screen: &mut Screen,
    context: Context<'_>,
    message: Message,
) -> Vec<Effect> {
    match message {
        Message::TabChanged(tab_id) => {
            screen.active_tab = tabs::get(tab_id);
            Vec::new()
        }
        Message::TranscodingTypeChanged(value) => {
            screen.working_config.video.transcoding_type = value;
            Vec::new()
        }
        Message::TargetResolutionChanged(value) => {
            screen.working_config.video.target_resolution = value;
            Vec::new()
        }
        Message::TargetFramerateChanged(value) => {
            screen.working_config.video.target_framerate = value;
            Vec::new()
        }
        Message::TargetBitrateChanged(value) => {
            screen.working_config.video.target_bitrate = value;
            Vec::new()
        }
        Message::TargetBitrateInputChanged(value) => {
            parse_bitrate(screen, value).into_iter().collect()
        }
        Message::RecordCursorChanged(value) => {
            screen.working_config.capture.record_cursor = value;
            Vec::new()
        }
        Message::RecordingBorderIndicatorChanged(value) => {
            screen.working_config.capture.recording_border_indicator = value;
            Vec::new()
        }
        Message::EnableUiPreviewChanged(value) => {
            screen.working_config.capture.enable_ui_preview = value;
            Vec::new()
        }
        Message::MaxDepacketLatencyChanged(value) => {
            screen.working_config.network.max_depacket_latency = clamp_max_depacket_latency(value);
            Vec::new()
        }
        Message::MaxDepacketLatencyInputChanged(value) => {
            parse_max_depacket_latency(screen, value).into_iter().collect()
        }
        Message::SaveSettings => {
            vec![Effect::SaveConfig(screen.working_config.clone())]
        }
        Message::DiscardSettings => {
            screen.working_config = context.config().clone();
            Vec::new()
        }
    }
}

fn parse_bitrate(screen: &mut Screen, value: String) -> Option<Effect> {
    match parse_target_bitrate_input(&value) {
        Ok(target_bitrate) => {
            screen.working_config.video.target_bitrate = target_bitrate;
            None
        }
        Err(message) => Some(Effect::NotifyError(message)),
    }
}

fn parse_max_depacket_latency(screen: &mut Screen, value: String) -> Option<Effect> {
    match parse_max_depacket_latency_input(&value) {
        Ok(latency) => {
            screen.working_config.network.max_depacket_latency = latency;
            None
        }
        Err(message) => Some(Effect::NotifyError(message)),
    }
}
