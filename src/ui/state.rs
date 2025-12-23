use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

use crate::{
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::{
        app::{ActiveScreen, PacketReceiverRef},
        notification_provider::NotificationProvider,
    },
};

pub struct AppContext {
    pub config: Config,

    pub packet_tx: Option<mpsc::Sender<Bytes>>,
    pub packet_rx: PacketReceiverRef,

    pub webrtc_event_tx: Option<mpsc::Sender<WebRTCEvent>>,
    pub webrtc_event_rx: Option<Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>>,

    pub main_window_handle: Option<u64>,

    pub webrtc: Option<WebRTC>,
    pub target_id: Option<String>,

    pub notifications: NotificationProvider,
}

pub struct State {
    pub ctx: AppContext,
    pub active_screen: ActiveScreen,
}
