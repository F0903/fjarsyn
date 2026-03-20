use super::{Config, PersistedConfig};
use crate::{
    capture_providers::CaptureFramerate,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
};

fn into_config(persisted: PersistedConfig) -> Config {
    match persisted {
        PersistedConfig::Current(config) => config,
        PersistedConfig::Legacy(config) => config.into(),
    }
}

#[test]
fn legacy_config_deserializes_into_nested_config() {
    let persisted: PersistedConfig = serde_json::from_str(
        r#"{
            "peer_id": "peer-123",
            "target_bitrate": 12000000,
            "target_framerate": "FPS60",
            "target_resolution": "Source",
            "pixel_format": "BGRA8",
            "max_depacket_latency": 750,
            "transcoding_type": "H264Nvenc",
            "record_cursor": false,
            "recording_border_indicator": true,
            "enable_ui_preview": false
        }"#,
    )
    .unwrap();

    let config = into_config(persisted);

    assert_eq!(config.identity.peer_id.as_deref(), Some("peer-123"));
    assert_eq!(config.video.target_bitrate, 12_000_000);
    assert_eq!(config.video.target_framerate, CaptureFramerate::FPS60);
    assert_eq!(config.video.target_resolution, TargetResolution::Source);
    assert_eq!(config.video.transcoding_type, FFmpegTranscodeType::H264Nvenc);
    assert_eq!(config.network.max_depacket_latency, 750);
    assert!(!config.capture.record_cursor);
    assert!(config.capture.recording_border_indicator);
    assert!(!config.capture.enable_ui_preview);
}

#[test]
fn current_config_round_trips() {
    let config = Config::default();
    let persisted: PersistedConfig =
        serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();

    assert_eq!(into_config(persisted), config);
}
