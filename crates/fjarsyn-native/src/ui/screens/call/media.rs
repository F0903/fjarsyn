use fjarsyn_core::media::pixel_format::PixelFormat;

use super::{
    CallScreen,
    workers::{CaptureWorker, CaptureWorkerConfig, EncoderWorker, EncoderWorkerConfig},
};
use crate::ui::app::AppContextMut;

impl CallScreen {
    pub(super) fn clear_media_pipeline(&mut self) {
        self.clear_local_media_pipeline();
        self.clear_remote_media_pipeline();
    }

    pub(super) fn clear_local_media_pipeline(&mut self) {
        self.local.reset();
    }

    pub(super) fn clear_remote_media_pipeline(&mut self) {
        self.remote.reset();
    }

    pub(super) fn start_local_capture_pipeline(
        &mut self,
        ctx: &mut AppContextMut<'_>,
    ) -> Result<(), String> {
        let Some(service) = &ctx.runtime.services.call_service else {
            return Err("Call service is not initialized yet.".into());
        };
        let Some(capture) = self.capture.provider.clone() else {
            return Err("Screen capture is not available.".into());
        };

        tracing::debug!(
            "Starting capture pipeline. target_fps_hz: {}, target_resolution: {:?}, target_bitrate: {}",
            ctx.config.video.target_framerate.to_hz(),
            ctx.config.video.target_resolution,
            ctx.config.video.target_bitrate
        );

        let encoder = EncoderWorker::start(EncoderWorkerConfig {
            capture: capture.clone(),
            webrtc: service.webrtc().clone(),
            target_bitrate: ctx.config.video.target_bitrate,
            target_fps_hz: ctx.config.video.target_framerate.to_hz(),
            target_resolution: ctx.config.video.target_resolution,
            input_format: PixelFormat::DEFAULT_CAPTURE,
            transcoding_type: ctx.config.video.transcoding_type,
        });

        self.local.encoder = Some(encoder.clone());
        self.local.capture_worker = Some(CaptureWorker::start(CaptureWorkerConfig {
            capture,
            framerate: ctx.config.video.target_framerate,
            encoder,
        }));

        Ok(())
    }
}
