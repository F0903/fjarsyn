//! Application-shell state, lifecycle, runtime ownership, and window state.

use std::time::Instant;

use fjarsyn_engine::{
    config::Config,
    contacts::{self, Contact},
    identity::PeerId,
    messaging, peer_session, presence, screen_share,
};

use crate::ui::{notification, presentation, runtime, subscription::Receiver};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::shell) enum Lifecycle {
    Starting,
    Ready,
    Failed(String),
    ShuttingDown,
    Restarting,
    RestartFailed(String),
}

/// Runtime owners and event channels are deliberately kept outside UI state.
pub(in crate::ui::shell) struct Runtime {
    pub(in crate::ui::shell) event_tx: tokio::sync::mpsc::Sender<runtime::Event>,
    pub(in crate::ui::shell) event_rx: Receiver<runtime::Event>,
    pub(in crate::ui::shell) application: Option<runtime::Application>,
}

pub(in crate::ui::shell) struct WindowInfo {
    pub(in crate::ui::shell) iced_id: iced::window::Id,
    pub(in crate::ui::shell) raw_id: Option<u64>,
    pub(in crate::ui::shell) maximized: bool,
}

pub(in crate::ui::shell) struct UiState {
    pub(in crate::ui::shell) main_window: Option<WindowInfo>,
    pub(in crate::ui::shell) notifications: notification::Center,
    pub(in crate::ui::shell) started_at: Instant,
}

pub(in crate::ui::shell) struct State {
    pub(in crate::ui::shell) config: Config,
    pub(in crate::ui::shell) lifecycle: Lifecycle,
    pub(in crate::ui::shell) local_peer_id: Option<PeerId>,
    pub(in crate::ui::shell) local_public_key: Option<String>,
    pub(in crate::ui::shell) contact_projection: Option<contacts::Projection>,
    pub(in crate::ui::shell) presence: presence::Snapshot,
    pub(in crate::ui::shell) sessions: peer_session::Snapshot,
    pub(in crate::ui::shell) messaging: messaging::Snapshot,
    pub(in crate::ui::shell) screen_share: screen_share::Snapshot,
    pub(in crate::ui::shell) ui: UiState,
}

impl State {
    pub(in crate::ui::shell) fn presentation(&self) -> presentation::Context<'_> {
        presentation::Context::new(presentation::Inputs {
            config: &self.config,
            local_peer_id: self.local_peer_id.as_ref(),
            local_public_key: self.local_public_key.as_deref(),
            contacts: self.contacts(),
            presence: &self.presence,
            sessions: &self.sessions,
            conversation_summaries: self.messaging.summaries.as_ref(),
            conversations: self.messaging.conversations.as_ref(),
            screen_share: &self.screen_share,
        })
    }

    pub(in crate::ui::shell) fn notify_error(&mut self, message: impl Into<String>) {
        self.ui.notifications.error(message);
    }

    pub(in crate::ui::shell) fn notify_info(&mut self, message: impl Into<String>) {
        self.ui.notifications.info(message);
    }

    pub(in crate::ui::shell) fn notify_success(&mut self, message: impl Into<String>) {
        self.ui.notifications.success(message);
    }

    pub(in crate::ui::shell) fn contacts(&self) -> &[Contact] {
        self.contact_projection.as_ref().map_or(&[], |projection| projection.contacts.as_ref())
    }

    pub(in crate::ui::shell) fn display_name(&self, peer_id: &PeerId) -> String {
        self.contacts()
            .iter()
            .find(|contact| &contact.peer_id == peer_id)
            .map(|contact| contact.name.clone())
            .unwrap_or_else(|| peer_id.to_string())
    }
}
