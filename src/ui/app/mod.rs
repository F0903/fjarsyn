use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use iced::{
    Alignment, Element, Length, Padding, Subscription, Task, Theme, padding,
    widget::{button, column, container, row, stack, text},
    window as iced_window,
};
use iced_fonts::lucide;
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::discovery::{Discovery, DiscoveryEvent, PeerInfo},
    services::{
        call_service::{CallEvent, CallService, CallServiceConfig},
        notification_service::NotificationService,
    },
    ui::{
        components,
        message::{Message, NotificationMessage},
        screens::{ActiveScreen, Screen},
        subscription::{self, EventReceiverRef},
        theme,
    },
};

pub const APP_TITLE: &str = "Fjarsyn";

pub mod handlers;

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
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
}

pub struct UIState {
    pub main_window: Option<WindowInfo>,
    pub back_queue: VecDeque<ActiveScreen>,
    pub notifications: NotificationService,
    pub started_at: std::time::Instant,
}

pub struct Services {
    pub call_service: Option<Arc<crate::services::call_service::CallService>>,
    pub contacts_service: Option<Arc<crate::services::contacts_service::ContactsService>>,
}

pub struct AppState {
    pub services: Services,
    pub networking: NetworkingState,
    pub media: MediaState,
    pub session: SessionState,
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

impl Fjarsyn {
    fn capture_cpu_readback_enabled(config: &Config) -> bool {
        crate::media::gpu_interop::requires_cpu_readback(
            config.enable_ui_preview,
            config.pixel_format,
            config.transcoding_type.get_encoder_info().hw_accel,
        )
    }

    pub fn init() -> (Self, Task<Message>) {
        let (frame_packet_tx, frame_packet_rx) = mpsc::channel(100);
        let (discovery_event_tx, discovery_event_rx) = mpsc::channel(100);
        let (call_event_tx, call_event_rx) = mpsc::channel(100);

        let config = Config::load().unwrap_or_else(|e| {
            tracing::error!("Failed to load config: {}", e);
            Config::default()
        });
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
                incoming_call_id: None,
                incoming_call_timeout: None,
            },
            ui: UIState {
                main_window: None,
                back_queue: VecDeque::new(),
                notifications: NotificationService::new(),
                started_at: std::time::Instant::now(),
            },
            config: config.clone(),
            db: None,
        };

        let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(&mut ctx));

        (
            Fjarsyn { ctx, active_screen },
            Task::batch([
                Task::future(async {
                    use crate::ui::message::DatabaseMessage;
                    Message::Database(DatabaseMessage::DatabaseInitialized(
                        crate::database::init().await.map_err(Arc::new),
                    ))
                }),
                Self::init_call_service_task(
                    frame_packet_tx,
                    call_event_tx,
                    discovery_event_tx,
                    config.max_depacket_latency,
                    config.peer_id,
                ),
                Self::open_window_task(),
                Self::load_fonts_task(),
            ]),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let screen_task = self.active_screen.update(&mut self.ctx, message.clone());

        let global_task = match message {
            Message::Navigation(msg) => handlers::navigation::handle_navigation_msg(self, msg),
            Message::WindowEvent(msg) => handlers::window::handle_window_event_msg(self, msg),
            Message::WindowControl(msg) => handlers::window::handle_window_control_msg(self, msg),
            Message::ContactData(msg) => handlers::contact::handle_contact_msg(self, msg),
            Message::CallService(msg) => handlers::service::handle_call_service_msg(self, msg),
            Message::Capture(msg) => handlers::service::handle_capture_msg(self, msg),
            Message::Database(msg) => handlers::service::handle_database_msg(self, msg),
            Message::CallAction(msg) => handlers::call_action::handle_call_action_msg(self, msg),
            Message::Notification(msg) => self.handle_notification_msg(msg),

            Message::CopyId(id) => {
                let id_clone = id.clone();
                Task::batch([
                    iced::clipboard::write(id_clone.clone()),
                    Task::done(Message::Notification(NotificationMessage::NotifyInfo(format!(
                        "Copied ID: {}",
                        id_clone
                    )))),
                ])
            }
            Message::Batch(messages) => {
                Task::batch(messages.into_iter().map(|msg| self.update(msg)))
            }
            Message::Tick(now) => self.handle_tick(now),

            _ => Task::none(),
        };

        Task::batch([screen_task, global_task])
    }

    fn handle_notification_msg(&mut self, message: NotificationMessage) -> Task<Message> {
        match message {
            NotificationMessage::DismissNotification(id) => {
                self.ctx.ui.notifications.dismiss(id);
                Task::none()
            }
            NotificationMessage::NotifyError(msg) => {
                self.ctx.notify_error(msg);
                Task::none()
            }
            NotificationMessage::NotifyInfo(msg) => {
                self.ctx.notify_info(msg);
                Task::none()
            }
            NotificationMessage::NotifySuccess(msg) => {
                self.ctx.notify_success(msg);
                Task::none()
            }
        }
    }

    fn handle_tick(&mut self, now: std::time::Instant) -> Task<Message> {
        self.ctx.ui.notifications.dismiss_expired(now);
        if self.ctx.session.incoming_call_timeout.is_some_and(|t| now >= t) {
            self.ctx.notify_info("Missed call.");
            use crate::ui::message::CallActionMessage;
            return Task::done(Message::CallAction(CallActionMessage::DeclineCall));
        }
        Task::none()
    }

    fn init_call_service_task(
        frame_packet_tx: mpsc::Sender<Bytes>,
        call_event_tx: mpsc::Sender<CallEvent>,
        discovery_event_tx: mpsc::Sender<DiscoveryEvent>,
        max_depacket_latency: u16,
        peer_id: Option<String>,
    ) -> Task<Message> {
        Task::future(async move {
            let config =
                CallServiceConfig { frame_packet_tx, call_event_tx, max_depacket_latency, peer_id };
            let res = CallService::init(config).await;
            if let Ok(ref service) = res
                && let Ok(d) = Discovery::new()
            {
                let _ = d.advertise(service.local_id(), service.signaling_port());
                let _ = d.browse(discovery_event_tx);
            }
            use crate::ui::message::CallServiceMessage;
            Message::CallService(CallServiceMessage::CallServiceInitialized(
                res.map(Arc::new).map_err(Arc::new),
            ))
        })
    }

    pub(crate) fn init_capture_task(config: &Config) -> Task<Message> {
        let fmt = config.pixel_format;
        let cursor = config.record_cursor;
        let border = config.recording_border_indicator;
        let cpu_readback_enabled = Self::capture_cpu_readback_enabled(config);
        Task::future(async move {
            let res = crate::capture_providers::windows::WgcCaptureProviderBuilder::new(
                fmt,
                cursor,
                border,
                cpu_readback_enabled,
            )
            .with_default_device()
            .and_then(|b| b.with_default_capture_item())
            .and_then(|b| b.build())
            .map(|p| Arc::new(RwLock::new(p)));
            use crate::ui::message::CaptureMessage;
            Message::Capture(CaptureMessage::CaptureInitialized(
                res.map_err(|e| Arc::new(crate::Error::from(e))),
            ))
        })
    }

    fn open_window_task() -> Task<Message> {
        use crate::ui::message::WindowEventMessage;
        iced_window::open(iced_window::Settings {
            decorations: false,
            #[cfg(target_os = "windows")]
            platform_specific: iced_window::settings::PlatformSpecific {
                undecorated_shadow: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .1
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)))
    }

    fn load_fonts_task() -> Task<Message> {
        use crate::ui::fonts::{geist, outfit};
        Task::batch([
            iced::font::load(iced_fonts::LUCIDE_FONT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BLACK_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BLACK_BYTES).map(|_| Message::NoOp),
        ])
    }

    fn incoming_call_popup<'a>(&self) -> Element<'a, Message> {
        let sender_id = match &self.ctx.session.incoming_call_id {
            Some(id) => id,
            None => {
                return column![].into();
            }
        };

        let sender_name = self
            .ctx
            .networking
            .discovered_peers
            .iter()
            .find(|p| p.id == *sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| {
                format!("{}...", crate::utils::string_utils::truncate(sender_id, 8))
            });

        use crate::ui::message::CallActionMessage;
        container(
            container(
                column![
                    text("Incoming Call").size(14).style(text::secondary),
                    text(sender_name).size(20).style(text::primary),
                    row![
                        button(row![lucide::phone_incoming().size(16), text("Accept")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::AcceptCall))
                            .style(button::success)
                            .padding(10),
                        button(row![lucide::phone_off().size(16), text("Decline")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::DeclineCall))
                            .style(button::danger)
                            .padding(10),
                    ]
                    .spacing(15)
                ]
                .spacing(15)
                .align_x(iced::Alignment::Center),
            )
            .padding(20)
            .style(theme::card_container),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Color { a: 0.8, ..iced::Color::BLACK }.into()),
            ..Default::default()
        })
        .into()
    }

    pub fn view<'a>(&'a self, _window: iced_window::Id) -> Element<'a, Message> {
        let screen_content = self.active_screen.view(&self.ctx);
        let current_route = self.active_screen.get_route();

        let titlebar = components::titlebar();
        let titlebar_size = match titlebar.as_widget().size().height {
            Length::Fixed(s) => s,
            _ => {
                tracing::warn!("Could not get titlebar_size in pixels!");
                0.0
            }
        };

        let main_content = container(screen_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::main_content_container);

        // Don't include sidebar in the call screen.
        let sidebar = match self.active_screen {
            ActiveScreen::Call(_) => None,
            _ => {
                let contacts = self.ctx.services.contacts_service.as_ref().map(|c| c.contacts());
                let contacts_ref = contacts.as_ref().map(|c| c.as_slice()).unwrap_or(&[]);

                Some(components::sidebar(
                    contacts_ref,
                    &self.ctx.networking,
                    current_route,
                    self.ctx.services.call_service.as_ref().map(|c| c.local_id().to_owned()),
                ))
            }
        };

        let mut main_layout = row![].padding(padding::top(titlebar_size));
        if let Some(sidebar) = sidebar {
            main_layout = main_layout.push(sidebar);
        }
        let main_layout = main_layout.push(main_content);

        let call_popup =
            self.ctx.session.incoming_call_id.is_some().then(|| self.incoming_call_popup());
        let mut call_popup_stack = stack![main_layout];
        if let Some(popup) = call_popup {
            call_popup_stack = call_popup_stack.push(popup);
        }

        let notifications =
            components::notifications_view(self.ctx.ui.notifications.notifications());
        let is_maximized = self.ctx.ui.main_window.as_ref().map(|w| w.maximized).unwrap_or(false);

        let controls = container(components::window_controls(is_maximized))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding(Padding::from([0, 15]))
            .align_x(Alignment::End)
            .align_y(Alignment::Center);

        let content_stack = stack![call_popup_stack, notifications, titlebar, controls];

        let final_stack = if is_maximized {
            content_stack
        } else {
            stack![content_stack, components::resize_grid()]
        };

        final_stack.into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        subscription::subscription(self)
    }

    pub fn theme(&self, _window: iced_window::Id) -> Theme {
        theme::fjarsyn_theme()
    }

    pub fn title(&self, _window: iced_window::Id) -> String {
        APP_TITLE.to_string()
    }
}
