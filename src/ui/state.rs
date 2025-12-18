use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

use crate::{
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::{
        app::{ActiveScreen, FrameReceiverRef},
        notification::{Notification, NotificationKind},
    },
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

    pub notifications: Vec<Notification>,
    pub notification_counter: u64,
}

impl AppContext {
    pub fn push_notification(&mut self, message: String, kind: NotificationKind) {
        self.notification_counter += 1;
        self.notifications.push(Notification::new(self.notification_counter, message, kind));
    }
}

pub struct State {
    pub ctx: AppContext,
    pub active_screen: ActiveScreen,
}
