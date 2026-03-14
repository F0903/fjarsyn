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
        call_service::{CallEvent, CallService, CallServiceConfig, DialSuccess},
        contact_service::ContactService,
        notification_service::NotificationService,
    },
    ui::{
        components,
        message::{Message, Route},
        screens::{ActiveScreen, Screen},
        subscription::{self, EventReceiverRef},
        theme,
    },
};

pub const APP_TITLE: &'static str = "Fjarsyn";

pub struct WindowInfo {
    pub iced_id: iced_window::Id,
    pub raw_id: Option<u64>,
    pub maximized: bool,
}

pub struct AppContext {
    pub config: Config,
    pub db: Option<sqlx::SqlitePool>,
    pub capture: Option<Arc<RwLock<PlatformCaptureProvider>>>,
    pub back_queue: VecDeque<ActiveScreen>,

    pub frame_packet_tx: Option<mpsc::Sender<Bytes>>,
    pub frame_packet_rx: EventReceiverRef<Bytes>,

    pub discovery_event_tx: Option<mpsc::Sender<DiscoveryEvent>>,
    pub discovery_event_rx: Option<Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>>,

    pub call_event_rx: Option<Arc<Mutex<mpsc::Receiver<CallEvent>>>>,

    pub main_window: Option<WindowInfo>,

    pub call_service: Option<Arc<CallService>>,
    pub target_id: Option<String>,
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
    pub discovered_peers: Vec<PeerInfo>,
    pub recent_peers: Vec<PeerInfo>,
    pub contacts: Vec<crate::database::Contact>,

    pub notifications: NotificationService,
}

impl AppContext {
    pub fn notify_error(&mut self, message: impl Into<String>) {
        self.notifications.error(message);
    }

    pub fn notify_info(&mut self, message: impl Into<String>) {
        self.notifications.info(message);
    }

    pub fn notify_success(&mut self, message: impl Into<String>) {
        self.notifications.success(message);
    }
}

pub struct Fjarsyn {
    pub(crate) ctx: AppContext,
    pub(crate) active_screen: ActiveScreen,
}

impl Fjarsyn {
    pub fn init() -> (Self, Task<Message>) {
        let (frame_packet_tx, frame_packet_rx) = mpsc::channel(100);
        let (discovery_event_tx, discovery_event_rx) = mpsc::channel(100);
        let (call_event_tx, call_event_rx) = mpsc::channel(100);

        let config = Config::load().unwrap_or_else(|e| {
            tracing::error!("Failed to load config: {}", e);
            Config::default()
        });
        let mut ctx = AppContext {
            config: config.clone(),
            db: None,
            capture: None,
            main_window: None,
            back_queue: VecDeque::new(),
            frame_packet_tx: Some(frame_packet_tx.clone()),
            frame_packet_rx: subscription::EventReceiverRef(Arc::new(Mutex::new(frame_packet_rx))),
            discovery_event_tx: Some(discovery_event_tx.clone()),
            discovery_event_rx: Some(Arc::new(Mutex::new(discovery_event_rx))),
            call_event_rx: Some(Arc::new(Mutex::new(call_event_rx))),
            call_service: None,
            target_id: None,
            incoming_call_id: None,
            incoming_call_timeout: None,
            discovered_peers: Vec::new(),
            recent_peers: Vec::new(),
            contacts: Vec::new(),
            notifications: NotificationService::new(),
        };

        let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(&mut ctx));

        (
            Fjarsyn { ctx, active_screen },
            Task::batch([
                Task::future(async {
                    Message::DatabaseInitialized(crate::database::init().await.map_err(Arc::new))
                }),
                Self::init_capture_task(&config),
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
        // 1. Immediate Delegation (Tunnel Down)
        let screen_task = self.active_screen.update(&mut self.ctx, message.clone());

        // 2. Global Orchestration & State Sync (Bubble Up)
        let global_task = match &message {
            Message::Navigate(route) => {
                self.active_screen = ActiveScreen::from_route(*route, &mut self.ctx);
                self.ctx.back_queue.clear();
                Task::none()
            }

            Message::NavigateWithBack(route) => {
                let mut screen = ActiveScreen::from_route(*route, &mut self.ctx);
                std::mem::swap(&mut self.active_screen, &mut screen);
                self.ctx.back_queue.push_front(screen);
                Task::none()
            }

            Message::Back => {
                if let Some(screen) = self.ctx.back_queue.pop_front() {
                    self.active_screen = screen;
                }
                Task::none()
            }

            Message::Tick(now) => self.handle_tick(*now),
            Message::DismissNotification(id) => {
                self.ctx.notifications.dismiss(*id);
                Task::none()
            }

            Message::NotifyError(msg) => {
                self.ctx.notify_error(msg);
                Task::none()
            }

            Message::NotifyInfo(msg) => {
                self.ctx.notify_info(msg);
                Task::none()
            }

            Message::NotifySuccess(msg) => {
                self.ctx.notify_success(msg);
                Task::none()
            }

            Message::CopyId(id) => {
                let id_clone = id.clone();
                Task::batch([
                    iced::clipboard::write(id_clone.clone()),
                    Task::done(Message::NotifyInfo(format!("Copied ID: {}", id_clone))),
                ])
            }

            // Window management
            Message::WindowOpened(id) => self.handle_window_opened(*id),
            Message::WindowClosed(id) => self.handle_window_closed(*id),
            Message::WindowMaximized(max) => {
                if let Some(w) = self.ctx.main_window.as_mut() {
                    w.maximized = *max;
                }
                Task::none()
            }

            Message::SyncMaximized => self
                .ctx
                .main_window
                .as_ref()
                .map(|w| iced_window::is_maximized(w.iced_id).map(Message::WindowMaximized))
                .unwrap_or(Task::none()),
            Message::WindowRawIdFetched((id, rid)) => self.handle_window_raw_id(*id, *rid),
            Message::Minimize => self.window_action(|id| iced_window::minimize(id, true)),
            Message::Maximize => self.window_action(iced_window::toggle_maximize),
            Message::Close => self.window_action(iced_window::close),
            Message::Drag => self.window_action(iced_window::drag),
            Message::Resize(dir) => self.window_action(|id| iced_window::drag_resize(id, *dir)),

            // Global Action Triggers (Using services directly to maintain isolation)
            Message::LoadContacts => {
                let Some(db) = self.ctx.db.clone() else { return Task::none() };
                Task::future(
                    async move { Message::ContactsLoaded(ContactService::list(&db).await) },
                )
            }

            Message::SaveContact { peer_id, name, address } => {
                let Some(db) = self.ctx.db.clone() else { return Task::none() };
                let peer_id = peer_id.clone();
                let name = name.clone();
                let address = address.clone();
                Task::future(async move {
                    Message::ContactSaved(ContactService::create(&db, peer_id, name, address).await)
                })
            }

            Message::DeleteContact(id) => {
                let Some(db) = self.ctx.db.clone() else { return Task::none() };
                let id = *id;
                Task::future(async move {
                    Message::ContactDeleted(ContactService::delete(&db, id).await)
                })
            }

            Message::UpdateContactAddress { id, new_address } => {
                if let Some(c) = self.ctx.contacts.iter().find(|c| c.id == *id) {
                    self.ctx.notify_info(format!("Updating address for {}...", c.name));
                    Task::done(Message::UpdateContactAddressConfirmed(*id, new_address.clone()))
                } else {
                    Task::none()
                }
            }

            Message::UpdateContactAddressConfirmed(id, addr) => {
                let Some(db) = self.ctx.db.clone() else { return Task::none() };
                let id = *id;
                let addr = addr.clone();
                let c = match self.ctx.contacts.iter().find(|c| c.id == id) {
                    Some(c) => c.clone(),
                    None => return Task::none(),
                };
                Task::future(async move {
                    let res = ContactService::update(&db, id, c.peer_id, c.name, Some(addr)).await;
                    match res {
                        Ok(_) => Message::LoadContacts,
                        Err(e) => Message::NotifyError(format!("Update Failed: {}", e)),
                    }
                })
            }

            Message::AcceptCall => {
                self.ctx.incoming_call_id = None;
                self.ctx.incoming_call_timeout = None;
                let Some(service) = self.ctx.call_service.clone() else { return Task::none() };
                Task::future(async move {
                    match service.accept().await {
                        Ok(_) => Message::Navigate(Route::Call),
                        Err(_) => Message::NoOp,
                    }
                })
            }

            Message::DeclineCall => {
                self.ctx.incoming_call_id = None;
                self.ctx.incoming_call_timeout = None;
                let Some(service) = self.ctx.call_service.clone() else { return Task::none() };
                Task::future(async move {
                    let _ = service.decline().await;
                    Message::NoOp
                })
            }

            Message::StartCall(target) => {
                let Some(service) = self.ctx.call_service.clone() else { return Task::none() };
                let target = target.clone();
                let discovered = self.ctx.discovered_peers.clone();
                let contacts = self.ctx.contacts.clone();
                Task::future(async move {
                    match service.dial(target, &contacts, &discovered).await {
                        Ok(DialSuccess { peer_id, name, socket_addr, update_contact_address }) => {
                            let mut batch = Vec::new();
                            if let Some((id, addr)) = update_contact_address {
                                batch.push(Message::UpdateContactAddress { id, new_address: addr });
                            }
                            if let (Some(id), Some(name), Some(addr)) = (peer_id, name, socket_addr)
                            {
                                batch.push(Message::PeerFound(PeerInfo {
                                    id,
                                    instance_name: name,
                                    host_name: "direct".into(),
                                    addresses: vec![addr.ip()],
                                    port: addr.port(),
                                }));
                            }
                            batch.push(Message::Navigate(Route::Call));
                            Message::Batch(batch)
                        }
                        Err(e_msg) => Message::NotifyError(e_msg),
                    }
                })
            }

            // Global State Synchronization & Orchestration
            Message::DatabaseInitialized(res) => match res {
                Ok(pool) => {
                    self.ctx.db = Some(pool.clone());
                    Task::done(Message::LoadContacts)
                }
                Err(e) => {
                    self.ctx.notify_error(format!("DB Failed: {}", e));
                    Task::none()
                }
            },

            Message::ContactSaved(res) => {
                if res.is_ok() {
                    self.ctx.notify_success("Contact saved.");
                    Task::done(Message::LoadContacts)
                } else {
                    Task::none()
                }
            }

            Message::ContactDeleted(res) => {
                if res.is_ok() {
                    self.ctx.notify_success("Contact deleted.");
                    Task::done(Message::LoadContacts)
                } else {
                    Task::none()
                }
            }

            Message::CallServiceInitialized(res) => {
                if let Ok(service) = res {
                    if self.ctx.config.peer_id.is_none() {
                        self.ctx.config.peer_id = Some(service.local_id().to_string());
                        let _ = self.ctx.config.save();
                    }
                    self.ctx.call_service = Some(service.clone());
                }
                Task::none()
            }

            Message::CallEvent(event) => {
                match event {
                    CallEvent::IncomingCall { peer_id } => {
                        self.ctx.target_id = Some(peer_id.clone());
                        self.ctx.incoming_call_id = Some(peer_id.clone());
                        self.ctx.incoming_call_timeout =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                    }
                    CallEvent::CallConnected => {
                        self.ctx.incoming_call_id = None;
                        self.ctx.incoming_call_timeout = None;
                        if let Some(tid) = &self.ctx.target_id {
                            if let Some(p) =
                                self.ctx.discovered_peers.iter().find(|p| p.id == *tid).cloned()
                            {
                                self.ctx.recent_peers.retain(|rp| rp.id != p.id);
                                self.ctx.recent_peers.insert(0, p);
                            }
                        }
                        return Task::done(Message::Navigate(Route::Call));
                    }
                    CallEvent::CallEnded => {
                        if self.ctx.target_id.is_some() {
                            self.ctx.notify_info("Call ended.");
                        }
                        self.ctx.target_id = None;
                        self.ctx.incoming_call_id = None;
                        self.ctx.incoming_call_timeout = None;
                    }
                }
                Task::none()
            }

            Message::CaptureInitialized(res) => self.handle_capture_init(res.clone()),

            Message::ContactsLoaded(res) => {
                if let Ok(c) = res {
                    self.ctx.contacts = c.clone();
                }
                Task::none()
            }

            Message::DiscoveryEvent(event) => {
                match event {
                    DiscoveryEvent::PeerFound(peer) => {
                        // Don't add ourselves
                        if self
                            .ctx
                            .call_service
                            .as_ref()
                            .map(|s| s.local_id() == peer.id)
                            .unwrap_or(false)
                        {
                            return Task::none();
                        }

                        if let Some(existing) =
                            self.ctx.discovered_peers.iter_mut().find(|p| p.id == peer.id)
                        {
                            existing.update(peer.clone());
                        } else {
                            self.ctx.discovered_peers.push(peer.clone());
                        }
                    }
                    DiscoveryEvent::PeerRemoved(fullname) => {
                        self.ctx.discovered_peers.retain(|p| !fullname.contains(&p.instance_name));
                    }
                }
                Task::none()
            }

            Message::PeerFound(peer) => {
                // Return if the peer is us
                if self.ctx.call_service.as_ref().map(|s| s.local_id() == peer.id).unwrap_or(false)
                {
                    return Task::none();
                }

                if let Some(existing) =
                    self.ctx.discovered_peers.iter_mut().find(|p| p.id == peer.id)
                {
                    existing.update(peer.clone());
                } else {
                    self.ctx.discovered_peers.push(peer.clone());
                }
                Task::none()
            }

            Message::PeerRemoved(id) => {
                self.ctx.discovered_peers.retain(|p| p.id != *id);
                Task::none()
            }

            Message::Batch(messages) => {
                Task::batch(messages.clone().into_iter().map(|msg| self.update(msg)))
            }

            _ => Task::none(),
        };

        Task::batch([screen_task, global_task])
    }

    fn handle_tick(&mut self, now: std::time::Instant) -> Task<Message> {
        self.ctx.notifications.dismiss_expired(now);
        if self.ctx.incoming_call_timeout.map_or(false, |t| now > t) {
            self.ctx.notify_info("Missed call.");
            return Task::done(Message::DeclineCall);
        }
        Task::none()
    }

    fn handle_capture_init(
        &mut self,
        res: Result<Arc<RwLock<PlatformCaptureProvider>>, Arc<crate::Error>>,
    ) -> Task<Message> {
        match res {
            Ok(provider) => {
                self.ctx.capture = Some(provider);
                tracing::info!("Capture ready.");
            }
            Err(e) => {
                self.ctx.notify_error(format!("Capture Failed: {}", e));
            }
        }
        Task::none()
    }

    fn handle_window_opened(&mut self, id: iced_window::Id) -> Task<Message> {
        if self.ctx.main_window.is_none() {
            self.ctx.main_window = Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
        }
        iced_window::raw_id::<Message>(id).map(move |rid| Message::WindowRawIdFetched((id, rid)))
    }

    fn handle_window_closed(&mut self, id: iced_window::Id) -> Task<Message> {
        if self.ctx.main_window.as_ref().map(|w| w.iced_id == id).unwrap_or(false) {
            self.ctx.main_window = None;
            return iced::exit();
        }
        Task::none()
    }

    fn handle_window_raw_id(&mut self, id: iced_window::Id, raw_id: u64) -> Task<Message> {
        if let Some(w) = self.ctx.main_window.as_mut().filter(|w| w.iced_id == id) {
            w.raw_id = Some(raw_id);
        }
        Task::none()
    }

    fn window_action(&self, f: impl FnOnce(iced_window::Id) -> Task<Message>) -> Task<Message> {
        self.ctx.main_window.as_ref().map(|w| f(w.iced_id)).unwrap_or(Task::none())
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
            if let Ok(ref service) = res {
                if let Ok(d) = Discovery::new() {
                    let _ = d.advertise(service.local_id(), service.signaling_port());
                    let _ = d.browse(discovery_event_tx);
                }
            }
            Message::CallServiceInitialized(res.map(Arc::new).map_err(Arc::new))
        })
    }

    fn init_capture_task(config: &Config) -> Task<Message> {
        let fmt = config.pixel_format;
        let cursor = config.record_cursor;
        let border = config.recording_border_indicator;
        Task::future(async move {
            let res = crate::capture_providers::windows::WgcCaptureProviderBuilder::new(
                fmt, cursor, border,
            )
            .with_default_device()
            .and_then(|b| b.with_default_capture_item())
            .and_then(|b| b.build())
            .map(|p| Arc::new(RwLock::new(p)));
            Message::CaptureInitialized(res.map_err(|e| Arc::new(crate::Error::from(e))))
        })
    }

    fn open_window_task() -> Task<Message> {
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
        .map(Message::WindowOpened)
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
        let sender_id = match &self.ctx.incoming_call_id {
            Some(id) => id,
            None => {
                return column![].into();
            }
        };

        let sender_name = self
            .ctx
            .discovered_peers
            .iter()
            .find(|p| p.id == *sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| {
                format!("{}...", crate::utils::string_utils::truncate(sender_id, 8))
            });

        container(
            container(
                column![
                    text("Incoming Call").size(14).style(text::secondary),
                    text(sender_name).size(20).style(text::primary),
                    row![
                        button(row![lucide::phone_incoming().size(16), text("Accept")].spacing(10))
                            .on_press(Message::AcceptCall)
                            .style(button::success)
                            .padding(10),
                        button(row![lucide::phone_off().size(16), text("Decline")].spacing(10))
                            .on_press(Message::DeclineCall)
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
            _ => Some(components::sidebar(&self.ctx, current_route)),
        };

        let mut main_layout = row![].padding(padding::top(titlebar_size));
        if let Some(sidebar) = sidebar {
            main_layout = main_layout.push(sidebar);
        }
        let main_layout = main_layout.push(main_content);

        let call_popup = self.ctx.incoming_call_id.is_some().then(|| self.incoming_call_popup());
        let mut call_popup_stack = stack![main_layout];
        if let Some(popup) = call_popup {
            call_popup_stack = call_popup_stack.push(popup);
        }

        let notifications = components::notifications_view(self.ctx.notifications.notifications());
        let is_maximized = self.ctx.main_window.as_ref().map(|w| w.maximized).unwrap_or(false);

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
