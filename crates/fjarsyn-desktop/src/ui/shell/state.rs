//! Application-shell state, lifecycle, runtime ownership, and window state.

use std::time::Instant;

use fjarsyn_engine::{
    contacts::{self, Contact},
    identity::PeerId,
    messaging, peer_session, presence, screen_share,
};

use crate::{
    settings::{PowerPreference, Settings, Store},
    ui::{notification, presentation, runtime},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::shell) enum Lifecycle {
    Starting,
    Ready,
    StartupFailed(String),
    Degraded(String),
    ShuttingDown,
    Restarting,
    RestartFailed(String),
}

/// Runtime ownership and ID admission kept outside presentation state.
pub(in crate::ui::shell) struct Runtime {
    pub(in crate::ui::shell) engine: Option<runtime::EngineRuntime>,
    expected_startup_id: Option<runtime::RuntimeId>,
    active_id: Option<runtime::RuntimeId>,
}

impl Lifecycle {
    pub(in crate::ui::shell) const fn accepts_engine_actions(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl Runtime {
    pub(in crate::ui::shell) const fn awaiting(runtime_id: runtime::RuntimeId) -> Self {
        Self { engine: None, expected_startup_id: Some(runtime_id), active_id: None }
    }

    pub(in crate::ui::shell) fn expect_new_startup(&mut self) -> runtime::RuntimeId {
        debug_assert!(self.engine.is_none());
        debug_assert!(self.expected_startup_id.is_none());
        debug_assert!(self.active_id.is_none());
        let runtime_id = runtime::RuntimeId::next();
        self.expected_startup_id = Some(runtime_id);
        self.active_id = None;
        runtime_id
    }

    pub(in crate::ui::shell) fn expects_startup(&self, runtime_id: runtime::RuntimeId) -> bool {
        self.expected_startup_id == Some(runtime_id)
    }

    pub(in crate::ui::shell) fn activate(&mut self, runtime_id: runtime::RuntimeId) {
        debug_assert!(self.expects_startup(runtime_id));
        self.expected_startup_id = None;
        self.active_id = Some(runtime_id);
    }

    pub(in crate::ui::shell) fn reject_startup(&mut self, runtime_id: runtime::RuntimeId) {
        if self.expects_startup(runtime_id) {
            self.expected_startup_id = None;
        }
    }

    pub(in crate::ui::shell) fn is_active(&self, runtime_id: runtime::RuntimeId) -> bool {
        self.active_id == Some(runtime_id)
    }

    pub(in crate::ui::shell) fn clear_ids(&mut self) {
        self.expected_startup_id = None;
        self.active_id = None;
    }

    #[cfg(test)]
    pub(in crate::ui::shell) const fn has_pending_startup(&self) -> bool {
        self.expected_startup_id.is_some()
    }
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
    pub(in crate::ui::shell) settings: Settings,
    pub(in crate::ui::shell) settings_store: Store,
    pub(in crate::ui::shell) active_power_preference: PowerPreference,
    pub(in crate::ui::shell) lifecycle: Lifecycle,
    pub(in crate::ui::shell) local_peer_id: Option<PeerId>,
    pub(in crate::ui::shell) local_public_key: Option<String>,
    pub(in crate::ui::shell) contact_projection: Option<contacts::Projection>,
    pub(in crate::ui::shell) presence: presence::NearbyPeers,
    pub(in crate::ui::shell) sessions: peer_session::Sessions,
    pub(in crate::ui::shell) messaging: messaging::Conversations,
    pub(in crate::ui::shell) screen_share: screen_share::Shares,
    pub(in crate::ui::shell) ui: UiState,
}

impl State {
    pub(in crate::ui::shell) fn presentation(&self) -> presentation::Context<'_> {
        presentation::Context::new(presentation::Inputs {
            settings: &self.settings,
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

#[cfg(test)]
mod tests {
    use super::{Lifecycle, Runtime};
    use crate::ui::runtime::RuntimeId;

    #[test]
    fn only_ready_lifecycle_accepts_engine_actions() {
        assert!(Lifecycle::Ready.accepts_engine_actions());
        for lifecycle in [
            Lifecycle::Starting,
            Lifecycle::StartupFailed("startup failed".into()),
            Lifecycle::Degraded("engine adapter failed".into()),
            Lifecycle::ShuttingDown,
            Lifecycle::Restarting,
            Lifecycle::RestartFailed("restart failed".into()),
        ] {
            assert!(!lifecycle.accepts_engine_actions());
        }
    }

    #[test]
    fn clearing_runtime_ids_revokes_startup_and_engine_output_admission() {
        let expected = RuntimeId::next();
        let mut awaiting = Runtime::awaiting(expected);
        assert!(awaiting.expects_startup(expected));
        awaiting.clear_ids();
        assert!(!awaiting.expects_startup(expected));

        let active = RuntimeId::next();
        let mut running = Runtime::awaiting(active);
        running.activate(active);
        assert!(running.is_active(active));
        running.clear_ids();

        assert!(!running.is_active(active));
    }
}
