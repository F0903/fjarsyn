use fjarsyn_core::{
    config::{Config, NetworkConfig},
    media::{ffmpeg::FFmpegTranscodeTypeExt, gpu_interop, pixel_format::PixelFormat},
};

use super::{SettingsMessage, SettingsScreen};
use crate::ui::app::AppContext;

#[derive(Debug, Clone)]
pub(crate) enum SettingsEffect {
    NotifyError(String),
    PersistConfig(Config),
    ApplyCaptureReadback { enabled: bool },
}

// The settings reducer keeps UI field mutations local and emits only the work
// that needs runtime access, such as persistence or capture reconfiguration.
pub(crate) fn reduce(
    screen: &mut SettingsScreen,
    ctx: AppContext<'_>,
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
            screen.working_config.network.max_depacket_latency =
                value.clamp(0, NetworkConfig::MAX_DEPACKET_LATENCY_MS);
            Vec::new()
        }
        SettingsMessage::MaxDepacketLatencyInputChanged(value) => {
            parse_max_depacket_latency(screen, value).into_iter().collect()
        }
        SettingsMessage::SaveSettings => {
            let config = screen.working_config.clone();
            vec![
                SettingsEffect::PersistConfig(config.clone()),
                SettingsEffect::ApplyCaptureReadback {
                    enabled: requires_capture_readback(&config),
                },
            ]
        }
        SettingsMessage::DiscardSettings => {
            screen.working_config = ctx.config.clone();
            Vec::new()
        }
    }
}

fn parse_bitrate(screen: &mut SettingsScreen, value: String) -> Option<SettingsEffect> {
    match value.parse::<u32>() {
        Ok(kbps) => {
            screen.working_config.video.target_bitrate = kbps * 1000;
            None
        }
        Err(_) => Some(SettingsEffect::NotifyError(format!("Invalid bitrate value: '{}'", value))),
    }
}

fn parse_max_depacket_latency(
    screen: &mut SettingsScreen,
    value: String,
) -> Option<SettingsEffect> {
    match value.parse::<u16>() {
        Ok(latency) => {
            screen.working_config.network.max_depacket_latency =
                latency.clamp(0, NetworkConfig::MAX_DEPACKET_LATENCY_MS);
            None
        }
        Err(_) => Some(SettingsEffect::NotifyError(format!(
            "Invalid max depacket latency value: '{}'",
            value
        ))),
    }
}

fn requires_capture_readback(config: &Config) -> bool {
    gpu_interop::requires_cpu_readback(
        config.capture.enable_ui_preview,
        PixelFormat::DEFAULT_CAPTURE,
        config.video.transcoding_type.get_encoder_info().hw_accel,
    )
}
