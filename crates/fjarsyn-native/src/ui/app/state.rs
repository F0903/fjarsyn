use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use bytes::Bytes;
pub use fjarsyn_core::app::{
    AppLifecycle, AppState, ContactsState, MessagingState, NetworkingState, ServicesState,
    SessionState,
};
use fjarsyn_core::{
    capture_providers::PlatformCaptureProvider,
    networking::discovery::DiscoveryEvent,
    services::{
        call_service::{CallEvent, CallService},
        contacts_service::ContactsService,
        discovery_service::DiscoveryService,
        messaging_service::{MessagingEvent, MessagingService},
        notification_service::NotificationService,
    },
};
use iced::window as iced_window;
use tokio::sync::{RwLock, mpsc};

use crate::ui::{
    screens::{ActiveScreen, ScreenEntry},
    subscription::EventReceiverRef,
};

pub const APP_TITLE: &str = "Fjarsyn";

pub struct WindowInfo {
    pub iced_id: iced_window::Id,
    pub raw_id: Option<u64>,
    pub maximized: bool,
}

pub struct MediaState {
    pub capture: Option<Arc<RwLock<PlatformCaptureProvider>>>,
    pub capture_initializing: bool,
}

pub struct UIState {
    pub main_window: Option<WindowInfo>,
    pub back_queue: VecDeque<ScreenEntry>,
    pub notifications: NotificationService,
    pub started_at: std::time::Instant,
    pub cursor_inside_window: bool,
}

pub struct RuntimeServices {
    pub call_service: Option<Arc<CallService>>,
    pub contacts_service: Option<Arc<ContactsService>>,
    pub discovery_service: Option<Arc<DiscoveryService>>,
    pub messaging_service: Option<Arc<MessagingService>>,
}

pub struct AppRuntime {
    pub frame_packet_tx: mpsc::Sender<Bytes>,
    pub frame_packet_rx: EventReceiverRef<Bytes>,
    pub discovery_event_tx: mpsc::Sender<DiscoveryEvent>,
    pub discovery_event_rx: EventReceiverRef<DiscoveryEvent>,
    pub call_event_tx: mpsc::Sender<CallEvent>,
    pub call_event_rx: EventReceiverRef<CallEvent>,
    pub messaging_event_tx: mpsc::Sender<MessagingEvent>,
    pub messaging_event_rx: EventReceiverRef<MessagingEvent>,
    pub services: RuntimeServices,
    pub db: Option<sqlx::SqlitePool>,
}

pub struct ShellState {
    pub core: AppState,
    pub media: MediaState,
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
}

impl Deref for ShellState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for ShellState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

pub struct Fjarsyn {
    pub(crate) ctx: ShellState,
    pub(crate) runtime: AppRuntime,
    pub(crate) active_screen: ActiveScreen,
}

#[derive(Clone, Copy)]
pub struct AppContextBase<State, Runtime> {
    pub state: State,
    pub runtime: Runtime,
}

pub type AppContext<'a> = AppContextBase<&'a ShellState, &'a AppRuntime>;
pub type AppContextMut<'a> = AppContextBase<&'a mut ShellState, &'a mut AppRuntime>;

impl<State, Runtime> AppContextBase<State, Runtime>
where
    Runtime: Deref<Target = AppRuntime>,
{
    pub fn services(&self) -> &RuntimeServices {
        &self.runtime.services
    }

    pub fn db(&self) -> Option<&sqlx::SqlitePool> {
        self.runtime.db.as_ref()
    }
}

impl<State, Runtime> Deref for AppContextBase<State, Runtime>
where
    State: Deref<Target = ShellState>,
{
    type Target = ShellState;

    fn deref(&self) -> &Self::Target {
        self.state.deref()
    }
}

impl<'a> AppContextBase<&'a mut ShellState, &'a mut AppRuntime> {
    pub fn as_ref(&self) -> AppContext<'_> {
        AppContextBase { state: &*self.state, runtime: &*self.runtime }
    }

    pub fn services_mut(&mut self) -> &mut RuntimeServices {
        &mut self.runtime.services
    }

    pub fn db_mut(&mut self) -> &mut Option<sqlx::SqlitePool> {
        &mut self.runtime.db
    }
}

impl<State, Runtime> DerefMut for AppContextBase<State, Runtime>
where
    State: DerefMut<Target = ShellState>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.deref_mut()
    }
}
