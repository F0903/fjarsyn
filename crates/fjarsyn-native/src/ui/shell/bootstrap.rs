use std::{collections::VecDeque, sync::Arc};

use fjarsyn_core::{
    config::Config, peer_session::PeerSessionServiceSnapshot, presence::PresenceSnapshot,
    services::notification_service::NotificationService,
};
use tokio::sync::mpsc;

use super::{
    ActiveScreen, AppLifecycle, Fjarsyn, MessagingState, ShellRuntime, ShellState, UIState,
};
use crate::ui::{runtime::MediaProjection, subscription::EventReceiverRef};

const RUNTIME_EVENT_CAPACITY: usize = 256;

impl Fjarsyn {
    pub(super) fn new(config: Config) -> Self {
        let (event_tx, event_rx) = mpsc::channel(RUNTIME_EVENT_CAPACITY);
        let ctx = ShellState {
            config,
            lifecycle: AppLifecycle::Starting,
            local_peer_id: None,
            local_public_key: None,
            contacts: Arc::new(Vec::new()),
            contacts_source_id: 0,
            contacts_revision: 0,
            presence: PresenceSnapshot::default(),
            sessions: PeerSessionServiceSnapshot::default(),
            messaging: MessagingState::default(),
            media: MediaProjection::default(),
            ui: UIState {
                main_window: None,
                back_queue: VecDeque::new(),
                notifications: NotificationService::new(),
                started_at: std::time::Instant::now(),
                cursor_inside_window: true,
            },
        };
        let runtime =
            ShellRuntime { event_tx, event_rx: EventReceiverRef::new(event_rx), application: None };
        let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new());

        Self { ctx, runtime, active_screen }
    }
}
