use std::{collections::VecDeque, sync::Arc};

use fjarsyn_core::{
    app::AppLifecycle, config::Config, services::notification_service::NotificationService,
};
use tokio::sync::mpsc;

use super::{
    ActiveScreen, AppContext, AppRuntime, AppState, ContactsState, Fjarsyn, MediaState,
    MessagingState, NetworkingState, RuntimeServices, ServicesState, SessionState, ShellState,
    UIState,
};
use crate::ui::subscription::EventReceiverRef;

const REMOTE_SAMPLE_QUEUE_CAPACITY: usize = 8;

pub(super) struct AppBootstrap {
    pub(super) app: Fjarsyn,
}

impl AppBootstrap {
    pub(super) fn new(config: Config) -> Self {
        // Build the long-lived runtime channels up front so startup tasks can
        // wire services together without mutating the UI state shape.
        let (frame_packet_tx, frame_packet_rx) = mpsc::channel(REMOTE_SAMPLE_QUEUE_CAPACITY);
        let (discovery_event_tx, discovery_event_rx) = mpsc::channel(100);
        let (call_event_tx, call_event_rx) = mpsc::channel(100);
        let (messaging_event_tx, messaging_event_rx) = mpsc::channel(100);

        let ctx = ShellState {
            core: AppState {
                networking: NetworkingState {
                    local_peer_id: None,
                    discovered_peers: Vec::new(),
                    recent_peers: Vec::new(),
                },
                session: SessionState {
                    target_id: None,
                    target_label: None,
                    incoming_call_id: None,
                    incoming_call_timeout: None,
                    call_connected: false,
                },
                messaging: MessagingState {
                    summaries: Arc::new(Vec::new()),
                    active_peer_id: None,
                    active_messages: Arc::new(Vec::new()),
                    revision: 0,
                },
                contacts: ContactsState { contacts: Arc::new(Vec::new()) },
                config,
                services: ServicesState::default(),
                lifecycle: AppLifecycle::Bootstrapping,
            },
            media: MediaState { capture: None, capture_initializing: false },
            ui: UIState {
                main_window: None,
                back_queue: VecDeque::new(),
                notifications: NotificationService::new(),
                started_at: std::time::Instant::now(),
                cursor_inside_window: true,
            },
        };

        let runtime = AppRuntime {
            frame_packet_tx,
            frame_packet_rx: EventReceiverRef::new(frame_packet_rx),
            discovery_event_tx,
            discovery_event_rx: EventReceiverRef::new(discovery_event_rx),
            call_event_tx,
            call_event_rx: EventReceiverRef::new(call_event_rx),
            messaging_event_tx,
            messaging_event_rx: EventReceiverRef::new(messaging_event_rx),
            services: RuntimeServices {
                call_service: None,
                contacts_service: None,
                discovery_service: None,
                messaging_service: None,
            },
            db: None,
        };

        let active_screen =
            ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(AppContext {
                state: &ctx,
                runtime: &runtime,
            }));

        Self { app: Fjarsyn { ctx, runtime, active_screen } }
    }
}
