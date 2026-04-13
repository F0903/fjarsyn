use std::sync::Arc;

use fjarsyn_core::{
    capture_providers::{CaptureProvider, PlatformCaptureProvider},
    media::{
        TargetResolution,
        ffmpeg::{FFmpegEncoder, FFmpegTranscodeType, FFmpegTranscodeTypeExt, HWAccelType},
        frame::Frame,
        pixel_format::PixelFormat,
    },
    networking::webrtc::WebRTC,
};
use tokio::sync::{RwLock, watch};

#[derive(Clone)]
pub(crate) struct EncoderWorker {
    frame_tx: watch::Sender<Option<Arc<Frame>>>,
}

impl std::fmt::Debug for EncoderWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncoderWorker")
    }
}

pub(crate) struct EncoderWorkerConfig {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,
    pub(crate) webrtc: Arc<WebRTC>,
    pub(crate) target_bitrate: u32,
    pub(crate) target_fps_hz: f32,
    pub(crate) target_resolution: TargetResolution,
    pub(crate) input_format: PixelFormat,
    pub(crate) transcoding_type: FFmpegTranscodeType,
}

impl EncoderWorker {
    pub(crate) fn start(config: EncoderWorkerConfig) -> Self {
        let (frame_tx, mut rx) = watch::channel::<Option<Arc<Frame>>>(None);

        tokio::spawn(async move {
            let device_handle =
                if config.transcoding_type.get_encoder_info().hw_accel == HWAccelType::D3D11VA {
                    config.capture.read().await.raw_device_handle()
                } else {
                    None
                };

            let mut encoder = match FFmpegEncoder::new(
                config.target_bitrate,
                config.target_fps_hz,
                config.target_resolution,
                config.input_format,
                device_handle,
                config.transcoding_type,
            ) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Failed to create encoder: {}", e);
                    return;
                }
            };

            loop {
                if rx.changed().await.is_err() {
                    break;
                }

                let Some(frame) = rx.borrow_and_update().clone() else {
                    continue;
                };

                match encoder.encode(&frame, config.transcoding_type, frame.size.x, frame.size.y) {
                    Ok(nal_units) => {
                        let frame_duration = match frame.duration {
                            Some(duration) => duration,
                            None => {
                                tracing::error!("Frame duration is None!");
                                continue;
                            }
                        };

                        for nal in nal_units {
                            if let Err(e) = config.webrtc.write_sample(nal, frame_duration).await {
                                tracing::error!("WebRTC write failed: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Encoding failed: {}", e);
                    }
                }
            }

            tracing::info!("Encoder worker finished.");
        });

        Self { frame_tx }
    }

    pub(crate) fn queue_frame(&self, frame: Arc<Frame>) {
        self.frame_tx.send_replace(Some(frame));
    }
}
