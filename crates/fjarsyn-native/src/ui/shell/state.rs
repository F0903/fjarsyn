use std::{collections::VecDeque, sync::Arc};

use fjarsyn_core::{
    config::Config,
    peer_session::{PeerId, PeerSessionServiceSnapshot, SessionId},
    presence::PresenceSnapshot,
    services::{
        contacts_service::Contact,
        messaging_service::{ConversationMessage, ConversationSummary},
        notification_service::NotificationService,
    },
};
use iced::window as iced_window;

use crate::ui::{
    runtime::{ApplicationRuntime, MediaProjection, RuntimeEvent},
    screens::{ActiveScreen, ScreenEntry},
    subscription::EventReceiverRef,
};

pub const APP_TITLE: &str = "Fjarsyn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppLifecycle {
    Starting,
    Ready,
    Failed(String),
    ShuttingDown,
}

#[derive(Debug, Clone)]
pub struct MessagingState {
    pub summaries: Arc<Vec<ConversationSummary>>,
    pub conversations: Arc<std::collections::BTreeMap<PeerId, Arc<Vec<ConversationMessage>>>>,
    pub revision: u64,
}

impl Default for MessagingState {
    fn default() -> Self {
        Self {
            summaries: Arc::new(Vec::new()),
            conversations: Arc::new(std::collections::BTreeMap::new()),
            revision: 0,
        }
    }
}

impl MessagingState {
    pub fn messages_for_peer(&self, peer_id: &PeerId) -> Arc<Vec<ConversationMessage>> {
        self.conversations.get(peer_id).cloned().unwrap_or_else(|| Arc::new(Vec::new()))
    }
}

pub struct WindowInfo {
    pub iced_id: iced_window::Id,
    pub raw_id: Option<u64>,
    pub maximized: bool,
}

pub struct UIState {
    pub main_window: Option<WindowInfo>,
    pub back_queue: VecDeque<ScreenEntry>,
    pub notifications: NotificationService,
    pub started_at: std::time::Instant,
    pub cursor_inside_window: bool,
}

/// Immutable application projections consumed by screens.
pub struct ShellState {
    pub config: Config,
    pub lifecycle: AppLifecycle,
    pub local_peer_id: Option<PeerId>,
    pub local_public_key: Option<String>,
    pub contacts: Arc<Vec<Contact>>,
    pub contacts_source_id: u64,
    pub contacts_revision: u64,
    pub presence: PresenceSnapshot,
    pub sessions: PeerSessionServiceSnapshot,
    pub messaging: MessagingState,
    pub media: MediaProjection,
    pub ui: UIState,
}

impl ShellState {
    pub fn notify_error(&mut self, message: impl Into<String>) {
        self.ui.notifications.error(message);
    }

    pub fn notify_info(&mut self, message: impl Into<String>) {
        self.ui.notifications.info(message);
    }

    pub fn notify_success(&mut self, message: impl Into<String>) {
        self.ui.notifications.success(message);
    }

    pub fn contact_for_peer(&self, peer_id: &PeerId) -> Option<&Contact> {
        self.contacts.iter().find(|contact| &contact.peer_id == peer_id)
    }

    pub fn display_name(&self, peer_id: &PeerId) -> String {
        self.contact_for_peer(peer_id)
            .map(|contact| contact.name.clone())
            .unwrap_or_else(|| peer_id.to_string())
    }

    pub fn is_nearby(&self, peer_id: &PeerId) -> bool {
        self.presence.is_nearby(peer_id.as_str())
    }

    pub fn connected_session_id(&self, peer_id: &PeerId) -> Option<SessionId> {
        self.sessions
            .session_for_peer(peer_id)
            .filter(|session| {
                session.phase == fjarsyn_core::peer_session::PeerSessionPhase::Connected
            })
            .map(|session| session.session_id)
    }
}

/// Runtime owners and event channels are deliberately kept outside UI state.
pub struct ShellRuntime {
    pub event_tx: tokio::sync::mpsc::Sender<RuntimeEvent>,
    pub event_rx: EventReceiverRef<RuntimeEvent>,
    pub application: Option<ApplicationRuntime>,
}

pub struct Fjarsyn {
    pub(crate) ctx: ShellState,
    pub(crate) runtime: ShellRuntime,
    pub(crate) active_screen: ActiveScreen,
}

#[derive(Clone, Copy)]
pub struct ShellContextBase<State> {
    state: State,
}

pub type ShellContext<'a> = ShellContextBase<&'a ShellState>;
pub type ShellContextMut<'a> = ShellContextBase<&'a mut ShellState>;

impl<'a> ShellContextBase<&'a ShellState> {
    pub fn new(state: &'a ShellState) -> Self {
        Self { state }
    }
}

impl<'a> ShellContextBase<&'a mut ShellState> {
    pub fn new_mut(state: &'a mut ShellState) -> Self {
        Self { state }
    }

    pub fn as_ref(&self) -> ShellContext<'_> {
        ShellContextBase { state: &*self.state }
    }
}

impl<State> std::ops::Deref for ShellContextBase<State>
where
    State: std::ops::Deref<Target = ShellState>,
{
    type Target = ShellState;

    fn deref(&self) -> &Self::Target {
        self.state.deref()
    }
}

impl<State> std::ops::DerefMut for ShellContextBase<State>
where
    State: std::ops::DerefMut<Target = ShellState>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.deref_mut()
    }
}
