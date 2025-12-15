use bytes::Bytes;

use crate::{
    capture_providers::shared::{CaptureFramerate, PixelFormat, Vector2},
    media::h264::{H264Decoder, H264Encoder},
    networking::webrtc::WebRTC,
    ui::{app::ActiveScreen, frame_receiver_ref::FrameReceiverRef},
};

pub struct State {
    pub active_screen: ActiveScreen,
    pub frame_receiver: FrameReceiverRef,

    pub active_window_handle: Option<u64>,
    pub capturing: bool,
    pub capture_frame_rate: CaptureFramerate,

    pub frame_data: Option<Bytes>,
    pub frame_dimensions: Vector2<i32>,
    pub frame_format: PixelFormat,

    pub encoder: Option<H264Encoder>,
    pub decoder: Option<H264Decoder>,

    pub webrtc: Option<Result<WebRTC, String>>,
    pub target_id: Option<String>,
}
