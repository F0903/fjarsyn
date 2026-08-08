use std::sync::Arc;

use fjarsyn_engine::{config::Config, messaging, peer_session, presence, screen_share};
use iced::Task;
use tokio::sync::mpsc;

use super::global;
use crate::ui::{
    message::{self, Message, Route},
    notification, runtime,
    screens::{Active, Screen},
    shell::{Lifecycle, Runtime, State, UiState},
    subscription::Receiver,
};

const RUNTIME_EVENT_CAPACITY: usize = 256;

pub(in crate::ui) struct Fjarsyn {
    pub(in crate::ui::shell) state: State,
    pub(in crate::ui::shell) runtime: Runtime,
    pub(in crate::ui::shell) active_screen: Active,
}

impl Fjarsyn {
    fn new(config: Config) -> Self {
        let (event_tx, event_rx) = mpsc::channel(RUNTIME_EVENT_CAPACITY);
        let state = State {
            config,
            lifecycle: Lifecycle::Starting,
            local_peer_id: None,
            local_public_key: None,
            contact_projection: None,
            presence: presence::Snapshot::default(),
            sessions: peer_session::Snapshot::default(),
            messaging: messaging::Snapshot::default(),
            screen_share: screen_share::Snapshot::default(),
            ui: UiState {
                main_window: None,
                notifications: notification::Center::new(),
                started_at: std::time::Instant::now(),
            },
        };
        let runtime = Runtime { event_tx, event_rx: Receiver::new(event_rx), application: None };
        let active_screen = Active::from_route(Route::Home, state.presentation());

        Self { state, runtime, active_screen }
    }

    pub(in crate::ui) fn init(config: Config) -> (Self, Task<Message>) {
        let app = Self::new(config.clone());
        let runtime = Self::start_runtime_task(config, app.runtime.event_tx.clone());
        (app, Task::batch([runtime, Self::open_window_task(), Self::load_fonts_task()]))
    }

    pub(in crate::ui::shell) fn start_runtime_task(
        config: Config,
        event_tx: mpsc::Sender<runtime::Event>,
    ) -> Task<Message> {
        Task::future(async move {
            let result = runtime::Application::start(config, event_tx)
                .await
                .map(runtime::Slot::new)
                .map_err(Arc::new);
            Message::Runtime(message::Runtime::Initialized(result))
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
        // Let the active screen update its own state first, then run any app-wide
        // orchestration that the message implies.
        let screen_task = {
            let context = self.state.presentation();
            self.active_screen.update(context, message.clone())
        };
        let global_task = global::handle_global_message(self, message);

        Task::batch([screen_task, global_task])
    }
}
