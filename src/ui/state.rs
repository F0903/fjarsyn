use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

use crate::{
    capture_providers::shared::{CaptureFramerate, Frame, PixelFormat, Vector2},
    config::Config,
    media::h264::H264Decoder,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::app::{ActiveScreen, FrameReceiverRef},
};

pub struct State {
    pub config: Config,
    pub pending_config: Option<Config>,
    pub active_screen: ActiveScreen,
    pub frame_receiver: FrameReceiverRef,
    pub webrtc_event_receiver: Option<Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>>,

    pub main_window_handle: Option<u64>,
    pub capturing: bool,
    pub capture_frame_rate: CaptureFramerate,

    pub frame_data: Option<Bytes>,
    pub frame_dimensions: Vector2<i32>,
    pub frame_format: PixelFormat,

    pub frame_sender: Option<mpsc::Sender<Arc<Frame>>>,
    pub decoder: Option<Arc<Mutex<H264Decoder>>>,

    pub webrtc: Option<Result<WebRTC, String>>,
    pub target_id: Option<String>,
}
