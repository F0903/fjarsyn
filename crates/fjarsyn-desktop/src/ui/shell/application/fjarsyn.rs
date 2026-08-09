use std::sync::Arc;

use fjarsyn_engine::{messaging, peer_session, presence, screen_share, settings as engine};
use iced::Task;

use crate::{
    settings::{Settings, Store},
    ui::{
        message::{self, Message, Route},
        notification, runtime,
        screens::Active,
        shell::{Lifecycle, Runtime, State, UiState, handlers},
    },
};

pub(in crate::ui) struct Fjarsyn {
    pub(in crate::ui::shell) state: State,
    pub(in crate::ui::shell) runtime: Runtime,
    pub(in crate::ui::shell) active_screen: Active,
}

impl Fjarsyn {
    pub(in crate::ui::shell) fn new(
        settings: Settings,
        settings_store: Store,
        expected_runtime_id: runtime::RuntimeId,
    ) -> Self {
        let state = State {
            active_power_preference: settings.power_preference,
            settings,
            settings_store,
            lifecycle: Lifecycle::Starting,
            local_peer_id: None,
            local_public_key: None,
            contact_projection: None,
            presence: presence::NearbyPeers::default(),
            sessions: peer_session::Sessions::default(),
            messaging: messaging::Conversations::default(),
            screen_share: screen_share::Shares::default(),
            ui: UiState {
                main_window: None,
                notifications: notification::Center::new(),
                started_at: std::time::Instant::now(),
            },
        };
        let runtime = Runtime::awaiting(expected_runtime_id);
        let active_screen = Active::from_route(Route::Home, state.presentation());

        Self { state, runtime, active_screen }
    }

    pub(in crate::ui) fn init(settings: Settings, settings_store: Store) -> (Self, Task<Message>) {
        let engine_settings = settings.engine.clone();
        let runtime_id = runtime::RuntimeId::next();
        let app = Self::new(settings, settings_store, runtime_id);
        let runtime = Self::start_runtime_task(runtime_id, engine_settings);
        (app, Task::batch([runtime, Self::open_window_task(), Self::load_fonts_task()]))
    }

    pub(in crate::ui::shell) fn start_runtime_task(
        runtime_id: runtime::RuntimeId,
        settings: engine::Settings,
    ) -> Task<Message> {
        Task::future(async move {
            let result = runtime::EngineRuntime::start(runtime_id, settings)
                .await
                .map(runtime::RuntimeSlot::new)
                .map_err(Arc::new);
            Message::Runtime(message::Runtime::Initialized { runtime_id, result })
        })
    }

    fn open_window_task() -> Task<Message> {
        iced::window::open(iced::window::Settings {
            decorations: false,
            min_size: Some(iced::Size::new(800.0, 600.0)),
            #[cfg(target_os = "windows")]
            platform_specific: iced::window::settings::PlatformSpecific {
                undecorated_shadow: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .1
        .map(|id| Message::WindowEvent(message::window::Event::WindowOpened(id)))
    }

    fn load_fonts_task() -> Task<Message> {
        use crate::ui::fonts;
        Task::batch([
            iced::font::load(iced_fonts::LUCIDE_FONT_BYTES).map(|_| Message::NoOp),
            iced::font::load(fonts::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(fonts::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(fonts::BOLD_BYTES).map(|_| Message::NoOp),
        ])
    }

    pub(in crate::ui) fn update(&mut self, message: Message) -> Task<Message> {
        let accepts_engine_actions = self.state.lifecycle.accepts_engine_actions();
        let settings_route_active = self.active_screen.is_settings();

        match message {
            Message::Navigation(message) => handlers::handle_navigation_msg(self, message),
            Message::Lifecycle(message) => handlers::handle_lifecycle_msg(self, message),
            Message::Settings(message) => {
                if accepts_settings_message(&self.state.lifecycle, settings_route_active, &message)
                {
                    handlers::handle_settings_msg(self, message)
                } else {
                    Task::none()
                }
            }
            Message::Screen(message) => {
                if accepts_screen_message(&self.state.lifecycle, settings_route_active, &message) {
                    let context = self.state.presentation();
                    self.active_screen.update(context, message)
                } else {
                    Task::none()
                }
            }
            Message::PeerAction(message) => {
                if accepts_engine_actions {
                    handlers::handle_peer_action(self, message)
                } else {
                    Task::none()
                }
            }
            Message::Runtime(message) => handlers::handle_runtime_msg(self, message),
            Message::Notification(message) => self.handle_notification(message),
            Message::ContactOperation(operation) => {
                if accepts_engine_actions {
                    handlers::handle_contact_operation(self, operation)
                } else {
                    Task::none()
                }
            }
            Message::WindowEvent(message) => handlers::handle_window_event_msg(self, message),
            Message::WindowControl(message) => handlers::handle_window_control_msg(self, message),
            Message::CopyId(id) => {
                let notice = format!("Copied ID: {id}");
                copy_task(id, notice)
            }
            Message::CopyInvite(invite) => copy_task(
                invite,
                "Copied pairing invite. The other person must import it and compare your full fingerprint.",
            ),
            Message::CopyFingerprint(fingerprint) => copy_task(
                fingerprint,
                "Copied the full identity fingerprint. Copying is only a convenience; compare it over an independent trusted channel before confirming.",
            ),
            Message::Tick(now) => {
                self.state.ui.notifications.dismiss_expired(now);
                Task::none()
            }
            Message::NoOp => Task::none(),
        }
    }

    fn handle_notification(&mut self, message: message::Notification) -> Task<Message> {
        match message {
            message::Notification::Dismiss(id) => self.state.ui.notifications.dismiss(id),
            message::Notification::NotifyError(message) => self.state.notify_error(message),
            message::Notification::NotifyInfo(message) => self.state.notify_info(message),
        }
        Task::none()
    }
}

fn accepts_settings_message(
    lifecycle: &Lifecycle,
    settings_route_active: bool,
    message: &message::Settings,
) -> bool {
    matches!((lifecycle, message), (Lifecycle::Ready, message::Settings::SaveRequested(_)))
        || matches!(lifecycle, Lifecycle::StartupFailed(_))
            && settings_route_active
            && matches!(message, message::Settings::SaveAndRetryRequested(_))
}

fn accepts_screen_message(
    lifecycle: &Lifecycle,
    settings_route_active: bool,
    message: &message::Screen,
) -> bool {
    match (lifecycle, message) {
        (
            Lifecycle::Ready,
            message::Screen::Settings(message::screen::settings::Message::SaveAndRetryStartup),
        ) => false,
        (Lifecycle::Ready, _) => true,
        (Lifecycle::StartupFailed(_), message::Screen::Settings(settings_message))
            if settings_route_active =>
        {
            !matches!(settings_message, message::screen::settings::Message::SaveSettings)
        }
        _ => false,
    }
}

fn copy_task(value: String, notice: impl Into<String>) -> Task<Message> {
    Task::batch([
        iced::clipboard::write(value),
        Task::done(Message::Notification(message::Notification::NotifyInfo(notice.into()))),
    ])
}

#[cfg(test)]
mod tests {
    use super::{accepts_screen_message, accepts_settings_message};
    use crate::{
        settings::Settings,
        ui::{
            message::{self, screen::settings::TabId},
            shell::Lifecycle,
        },
    };

    #[test]
    fn startup_failure_admits_only_the_settings_recovery_message_family() {
        let failed = Lifecycle::StartupFailed("failed".into());
        let settings_screen = message::Screen::Settings(
            message::screen::settings::Message::TabChanged(TabId::Network),
        );
        let ordinary_save_screen =
            message::Screen::Settings(message::screen::settings::Message::SaveSettings);
        let peer_screen = message::Screen::Peer(message::screen::PeerMessage::SendPressed);
        let retry = message::Settings::SaveAndRetryRequested(Settings::default());
        let ordinary_save = message::Settings::SaveRequested(Settings::default());

        assert!(accepts_screen_message(&failed, true, &settings_screen));
        assert!(!accepts_screen_message(&failed, true, &ordinary_save_screen));
        assert!(!accepts_screen_message(&failed, true, &peer_screen));
        assert!(!accepts_screen_message(&failed, false, &settings_screen));
        assert!(accepts_settings_message(&failed, true, &retry));
        assert!(!accepts_settings_message(&failed, true, &ordinary_save));
    }

    #[test]
    fn ready_runtime_admits_normal_screen_and_settings_messages_only() {
        let settings_screen = message::Screen::Settings(
            message::screen::settings::Message::TabChanged(TabId::Capture),
        );
        let recovery_screen =
            message::Screen::Settings(message::screen::settings::Message::SaveAndRetryStartup);
        let ordinary_save = message::Settings::SaveRequested(Settings::default());
        let retry = message::Settings::SaveAndRetryRequested(Settings::default());

        assert!(accepts_screen_message(&Lifecycle::Ready, false, &settings_screen));
        assert!(!accepts_screen_message(&Lifecycle::Ready, false, &recovery_screen));
        assert!(accepts_settings_message(&Lifecycle::Ready, false, &ordinary_save));
        assert!(!accepts_settings_message(&Lifecycle::Ready, false, &retry));
        assert!(!accepts_settings_message(&Lifecycle::Ready, true, &retry));
    }
}
