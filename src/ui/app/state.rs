use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use iced::window as iced_window;
use tokio::sync::{Mutex, RwLock, mpsc};

use super::ActiveScreen;
use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::discovery::{DiscoveryEvent, PeerInfo},
    services::{
        call_service::{CallEvent, CallService},
        contacts_service::ContactsService,
        messaging_service::{MessagingEvent, MessagingService},
        notification_service::NotificationService,
    },
    ui::subscription::EventReceiverRef,
};

pub const APP_TITLE: &str = "Fjarsyn";

pub struct WindowInfo {
    pub iced_id: iced_window::Id,
    pub raw_id: Option<u64>,
    pub maximized: bool,
}

pub struct NetworkingState {
    pub discovered_peers: Vec<PeerInfo>,
    pub recent_peers: Vec<PeerInfo>,
    pub discovery_event_tx: Option<mpsc::Sender<DiscoveryEvent>>,
    pub discovery_event_rx: Option<Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>>,
    pub call_event_rx: Option<Arc<Mutex<mpsc::Receiver<CallEvent>>>>,
}

pub struct MediaState {
    pub capture: Option<Arc<RwLock<PlatformCaptureProvider>>>,
    pub capture_initializing: bool,
    pub frame_packet_tx: Option<mpsc::Sender<Bytes>>,
    pub frame_packet_rx: EventReceiverRef<Bytes>,
}

pub struct SessionState {
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
    pub call_connected: bool,
}

pub struct MessagingState {
    pub event_tx: Option<mpsc::Sender<MessagingEvent>>,
    pub event_rx: Option<Arc<Mutex<mpsc::Receiver<MessagingEvent>>>>,
    pub pending_open_peer_id: Option<String>,
}

pub struct UIState {
    pub main_window: Option<WindowInfo>,
    pub back_queue: VecDeque<ActiveScreen>,
    pub notifications: NotificationService,
    pub started_at: std::time::Instant,
    pub cursor_inside_window: bool,
}

pub struct Services {
    pub call_service: Option<Arc<CallService>>,
    pub contacts_service: Option<Arc<ContactsService>>,
    pub messaging_service: Option<Arc<MessagingService>>,
}

pub struct AppState {
    pub services: Services,
    pub networking: NetworkingState,
    pub media: MediaState,
    pub session: SessionState,
    pub messaging: MessagingState,
    pub ui: UIState,
    pub config: Config,
    pub db: Option<sqlx::SqlitePool>,
}

impl AppState {
    pub fn notify_error(&mut self, message: impl Into<String>) {
        self.ui.notifications.error(message);
    }

    pub fn notify_info(&mut self, message: impl Into<String>) {
        self.ui.notifications.info(message);
    }

    pub fn notify_success(&mut self, message: impl Into<String>) {
        self.ui.notifications.success(message);
    }
}

pub struct Fjarsyn {
    pub(crate) ctx: AppState,
    pub(crate) active_screen: ActiveScreen,
}
