use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use tokio::sync::{Mutex, mpsc};

use super::{
    ActiveScreen, AppState, Fjarsyn, MediaState, NetworkingState, Services, SessionState, UIState,
};
use crate::{
    config::Config,
    networking::discovery::DiscoveryEvent,
    services::{call_service::CallEvent, notification_service::NotificationService},
    ui::subscription,
};

const REMOTE_SAMPLE_QUEUE_CAPACITY: usize = 8;

pub(super) struct AppBootstrap {
    pub(super) app: Fjarsyn,
    pub(super) runtime: AppRuntime,
}

pub(super) struct AppRuntime {
    pub(super) frame_packet_tx: mpsc::Sender<Bytes>,
    pub(super) discovery_event_tx: mpsc::Sender<DiscoveryEvent>,
    pub(super) call_event_tx: mpsc::Sender<CallEvent>,
    pub(super) max_depacket_latency: u16,
    pub(super) peer_id: Option<String>,
}

impl AppBootstrap {
    pub(super) fn load() -> Self {
        let config = Config::load().unwrap_or_else(|e| {
            tracing::error!("Failed to load config: {}", e);
            Config::default()
        });

        Self::new(config)
    }

    pub(super) fn new(config: Config) -> Self {
        // Build the long-lived app state and runtime channels up front so the
        // Iced task layer only has to kick off async initialization work.
        let (frame_packet_tx, frame_packet_rx) = mpsc::channel(REMOTE_SAMPLE_QUEUE_CAPACITY);
        let (discovery_event_tx, discovery_event_rx) = mpsc::channel(100);
        let (call_event_tx, call_event_rx) = mpsc::channel(100);

        let mut ctx = AppState {
            services: Services { call_service: None, contacts_service: None },
            networking: NetworkingState {
                discovered_peers: Vec::new(),
                recent_peers: Vec::new(),
                discovery_event_tx: Some(discovery_event_tx.clone()),
                discovery_event_rx: Some(Arc::new(Mutex::new(discovery_event_rx))),
                call_event_rx: Some(Arc::new(Mutex::new(call_event_rx))),
            },
            media: MediaState {
                capture: None,
                capture_initializing: false,
                frame_packet_tx: Some(frame_packet_tx.clone()),
                frame_packet_rx: subscription::EventReceiverRef(Arc::new(Mutex::new(
                    frame_packet_rx,
                ))),
            },
            session: SessionState {
                target_id: None,
                target_label: None,
                incoming_call_id: None,
                incoming_call_timeout: None,
                call_connected: false,
            },
            ui: UIState {
                main_window: None,
                back_queue: VecDeque::new(),
                notifications: NotificationService::new(),
                started_at: std::time::Instant::now(),
                cursor_inside_window: true,
            },
            config: config.clone(),
            db: None,
        };

        let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(&mut ctx));

        Self {
            app: Fjarsyn { ctx, active_screen },
            runtime: AppRuntime {
                frame_packet_tx,
                discovery_event_tx,
                call_event_tx,
                max_depacket_latency: config.network.max_depacket_latency,
                peer_id: config.identity.peer_id,
            },
        }
    }
}
