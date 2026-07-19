mod blocking_gate;
mod scripted_codec_backend_factory;

use std::{sync::Arc, time::Duration};

pub(super) use blocking_gate::{BlockingGate, ReleaseGateOnDrop};
use bytes::Bytes;
pub(super) use scripted_codec_backend_factory::{
    DecoderPlan, EncoderPlan, ScriptedCodecBackendFactory,
};

use super::super::{Config, DecoderWorkerConfig, EncoderWorkerConfig, Handle, Service};
use crate::{
    media::{
        FFmpegTranscodeType, TargetResolution,
        frame::{Frame, FrameData},
        pixel_format::PixelFormat,
    },
    utils::vector2::Vector2,
};

pub(super) fn test_service(factory: ScriptedCodecBackendFactory) -> (Service, Handle) {
    Service::start_with_backend(
        Config {
            call_timeout: Duration::from_millis(50),
            stop_timeout: Duration::from_millis(100),
        },
        Arc::new(factory),
    )
}

pub(super) fn encoder_config() -> EncoderWorkerConfig {
    EncoderWorkerConfig {
        bitrate: 1_000_000,
        target_framerate_hz: 30.0,
        target_resolution: TargetResolution::Source,
        input_format: PixelFormat::BGRA8,
        device: None,
        transcoding_type: FFmpegTranscodeType::H264Software,
    }
}

pub(super) fn decoder_config() -> DecoderWorkerConfig {
    DecoderWorkerConfig {
        transcoding_type: FFmpegTranscodeType::H264Software,
        output_format: PixelFormat::BGRA8,
    }
}

pub(super) fn test_frame() -> Arc<Frame> {
    Arc::new(Frame {
        data: FrameData::Software(Bytes::from_static(&[0, 0, 0, 0])),
        format: PixelFormat::BGRA8,
        size: Vector2::new(1, 1),
        duration: Some(Duration::from_millis(16)),
    })
}

pub(super) async fn wait_until_reaped(handle: &Handle) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if handle.worker_count_for_test() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("codec worker was not reaped");
}
