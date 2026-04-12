use std::sync::Arc;

use fjarsyn_core::{
    capture_providers::{CaptureProvider, PlatformCaptureProvider},
    media::{ffmpeg::FFmpegTranscodeType, frame::Frame, pixel_format::PixelFormat},
};
use tokio::sync::RwLock;

use super::workers::{CaptureWorker, DecoderWorker, EncoderWorker, LatestFrameReceiverRef};
use crate::ui::subscription::EventReceiverRef;

#[derive(Clone, Debug)]
pub(crate) struct CaptureState {
    pub(crate) provider: Option<Arc<RwLock<PlatformCaptureProvider>>>,
    pub(crate) pending_start: bool,
}

impl CaptureState {
    pub(crate) fn new(provider: Option<Arc<RwLock<PlatformCaptureProvider>>>) -> Self {
        Self { provider, pending_start: false }
    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.provider
            .as_ref()
            .and_then(|capture| capture.try_read().ok())
            .map(|capture| capture.is_capturing())
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LocalShareState {
    pub(crate) latest_frame: Option<Arc<Frame>>,
    pub(crate) preview_visible: bool,
    pub(crate) capture_worker: Option<CaptureWorker>,
    pub(crate) encoder: Option<EncoderWorker>,
}

impl LocalShareState {
    pub(crate) fn reset(&mut self) {
        self.latest_frame = None;
        self.preview_visible = false;
        self.capture_worker = None;
        self.encoder = None;
    }

    pub(crate) fn latest_frame_receiver(&self) -> Option<LatestFrameReceiverRef> {
        self.capture_worker.as_ref().map(CaptureWorker::frame_receiver)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteVideoState {
    pub(crate) latest_frame: Option<Arc<Frame>>,
    pub(crate) decoder: Option<DecoderWorker>,
    pub(crate) stream_status: RemoteStreamStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RemoteStreamStatus {
    #[default]
    Unknown,
    Active,
    Inactive,
}

impl RemoteVideoState {
    pub(crate) fn new(
        packet_receiver: EventReceiverRef<bytes::Bytes>,
        transcoding_type: FFmpegTranscodeType,
        pixel_format: PixelFormat,
    ) -> Self {
        Self {
            latest_frame: None,
            decoder: Some(DecoderWorker::start(
                super::workers::DecoderWorkerConfig { transcoding_type, pixel_format },
                packet_receiver,
            )),
            stream_status: RemoteStreamStatus::Unknown,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.latest_frame = None;
        self.decoder = None;
        self.stream_status = RemoteStreamStatus::Unknown;
    }

    pub(crate) fn decoded_frame_receiver(&self) -> Option<LatestFrameReceiverRef> {
        self.decoder.as_ref().map(DecoderWorker::frame_receiver)
    }
}
