use std::{sync::Arc, time::Duration};

use bytes::Bytes;

use super::ScriptedBackendFactory;
use crate::{
    media::{
        Dimensions, PixelFormat,
        codec::{
            CodecService, Config, DecoderWorkerConfig, EncoderWorkerConfig, ServiceHandle,
            TranscodeType,
        },
        frame::{Frame, FrameData},
        video::TargetResolution,
    },
    service_host::HostedService,
};

pub(in crate::media::codec::tests) fn test_service(
    factory: ScriptedBackendFactory,
) -> (CodecService, ServiceHandle) {
    let service = CodecService::start_with_backend(
        Config {
            call_timeout: Duration::from_millis(50),
            stop_timeout: Duration::from_millis(100),
        },
        Arc::new(factory),
    );
    let handle = service.service_handle();
    (service, handle)
}

pub(in crate::media::codec::tests) fn encoder_config() -> EncoderWorkerConfig {
    EncoderWorkerConfig {
        bitrate: 1_000_000,
        target_framerate_hz: 30.0,
        target_resolution: TargetResolution::Source,
        device: None,
        transcoding_type: TranscodeType::H264Software,
    }
}

pub(in crate::media::codec::tests) fn decoder_config() -> DecoderWorkerConfig {
    DecoderWorkerConfig {
        transcoding_type: TranscodeType::H264Software,
        output_format: PixelFormat::BGRA8,
    }
}

pub(in crate::media::codec::tests) fn test_frame() -> Arc<Frame> {
    Arc::new(Frame {
        data: FrameData::Software(Bytes::from_static(&[0, 0, 0, 0])),
        format: PixelFormat::BGRA8,
        size: Dimensions::new(1, 1),
        duration: Some(Duration::from_millis(16)),
    })
}

pub(in crate::media::codec::tests) async fn wait_until_reaped(handle: &ServiceHandle) {
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
