use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::{
        discovery::{DiscoveryEvent, PeerInfo},
        webrtc::{WebRTC, WebRTCEvent},
    },
    ui::{
        notification_provider::NotificationProvider, screens::ActiveScreen,
        subscription::EventReceiverRef,
    },
};

pub struct WindowInfo {
    pub iced_id: iced::window::Id,
    pub raw_id: Option<u64>,
    pub maximized: bool,
}

pub struct AppContext {
    pub config: Config,

    pub capture: Arc<RwLock<PlatformCaptureProvider>>,

    pub back_queue: VecDeque<ActiveScreen>,

    pub packet_tx: Option<mpsc::Sender<Bytes>>,
    pub packet_rx: EventReceiverRef<Bytes>,

    pub webrtc_event_tx: Option<mpsc::Sender<WebRTCEvent>>,
    pub webrtc_event_rx: Option<Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>>,

    pub discovery_event_tx: Option<mpsc::Sender<DiscoveryEvent>>,
    pub discovery_event_rx: Option<Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>>,

    pub main_window: Option<WindowInfo>,

    pub webrtc: Option<WebRTC>,
    pub target_id: Option<String>,
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
    pub discovered_peers: Vec<PeerInfo>,
    pub recent_peers: Vec<PeerInfo>,

    pub notifications: NotificationProvider,
}

pub struct State {
    pub ctx: AppContext,
    pub active_screen: ActiveScreen,
}
