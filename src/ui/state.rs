use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

use crate::{
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::app::{ActiveScreen, FrameReceiverRef},
};

pub struct AppContext {
    pub config: Config,

    pub frame_tx: Option<mpsc::Sender<Bytes>>,
    pub frame_rx: FrameReceiverRef,

    pub webrtc_event_tx: Option<mpsc::Sender<WebRTCEvent>>,
    pub webrtc_event_rx: Option<Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>>,

    pub main_window_handle: Option<u64>,

    pub webrtc: Option<Result<WebRTC, String>>,
    pub target_id: Option<String>,
}

pub struct State {
    pub ctx: AppContext,
    pub active_screen: ActiveScreen,
}
