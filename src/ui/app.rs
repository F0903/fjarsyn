use std::{collections::VecDeque, sync::Arc};

use bytes::Bytes;
use futures::stream::unfold;
use iced::{Element, Subscription, Task, window};
use tokio::sync::{Mutex, RwLock, mpsc};

use super::screens::{self, Screen};
use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::{
        message::{Message, Route},
        notification_provider::NotificationProvider,
        state::{AppContext, State, WindowInfo},
    },
};

#[derive(Debug, Clone)]
pub enum ActiveScreen {
    Onboarding(screens::onboarding::OnboardingScreen),
    Home(screens::home::HomeScreen),
    Call(screens::call::CallScreen),
    Settings(screens::settings::SettingsScreen),
}

#[derive(Clone)]
pub struct App {}

// Wrapper to implement Hash which is needed by iced subscriptions.
#[derive(Clone)]
struct WebRTCEventReceiverRef(Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>);

impl std::hash::Hash for WebRTCEventReceiverRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl PartialEq for WebRTCEventReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for WebRTCEventReceiverRef {}

fn webrtc_event_subscription_stream(
    receiver_ref: &WebRTCEventReceiverRef,
) -> Box<dyn futures::Stream<Item = Message> + Send + Unpin> {
    let receiver = receiver_ref.0.clone();
    Box::new(Box::pin(unfold(
        receiver,
        |receiver: Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>| async move {
            let mut lock = receiver.lock().await;
            if let Some(event) = lock.recv().await {
                drop(lock);
                Some((Message::WebRTCEvent(event), receiver))
            } else {
                drop(lock);
                None
            }
        },
    )))
}

// Wrapper to implement Hash which is needed by iced subscriptions.
#[derive(Clone)]
pub struct PacketReceiverRef(pub Arc<Mutex<mpsc::Receiver<Bytes>>>);

impl std::hash::Hash for PacketReceiverRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl PartialEq for PacketReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PacketReceiverRef {}

fn frame_subscription_stream(
    receiver_ref: &PacketReceiverRef,
) -> Box<dyn futures::Stream<Item = Message> + Send + Unpin> {
    let receiver = receiver_ref.0.clone();
    Box::new(Box::pin(unfold(receiver, |receiver| async move {
        let mut lock = receiver.lock().await;
        if let Some(packet) = lock.recv().await {
            drop(lock);
            Some((Message::PacketReceived(packet), receiver))
        } else {
            drop(lock);
            None
        }
    })))
}

impl App {
    const APP_TITLE: &'static str = "Fjarsyn";

    pub fn title(_state: &State, _window: window::Id) -> String {
        Self::APP_TITLE.to_string()
    }

    pub fn init(capture: Arc<RwLock<PlatformCaptureProvider>>) -> (State, Task<Message>) {
        const REMOTE_FRAMES_BUFFER: usize = 100;
        const WEBRTC_EVENT_BUFFER: usize = 100;
        let (frame_tx, frame_rx) = mpsc::channel(REMOTE_FRAMES_BUFFER);
        let (event_tx, event_rx) = mpsc::channel(WEBRTC_EVENT_BUFFER);

        let config = Config::load();
        let server_url = config.server_url.clone();

        let onboarding_done = config.onboarding_done;

        // Clone for potential init task
        let init_frame_tx = frame_tx.clone();
        let init_event_tx = event_tx.clone();

        let mut ctx = AppContext {
            config,
            capture,
            main_window: None,

            back_queue: VecDeque::new(),

            packet_tx: Some(frame_tx),
            packet_rx: PacketReceiverRef(Arc::new(Mutex::new(frame_rx))),

            webrtc_event_tx: Some(event_tx),
            webrtc_event_rx: Some(Arc::new(Mutex::new(event_rx))),

            webrtc: None,
            target_id: None,

            notifications: NotificationProvider::new(),
        };

        let active_screen = if onboarding_done {
            ActiveScreen::Home(screens::home::HomeScreen::new(&mut ctx))
        } else {
            ActiveScreen::Onboarding(screens::onboarding::OnboardingScreen::new(server_url.clone()))
        };

        let init_task = if onboarding_done {
            Task::future(async move {
                WebRTC::init(
                    server_url,
                    init_frame_tx,
                    init_event_tx,
                    ctx.config.max_depacket_latency,
                )
                .await
            })
            .map_err(Arc::new)
            .map(Message::WebRTCInitialized)
        } else {
            Task::none()
        };

        let window_settings =
            window::Settings { visible: true, transparent: true, ..Default::default() };

        let (_id, open_window_task) = window::open(window_settings);
        let open_window_task = open_window_task.map(Message::WindowOpened);

        (State { ctx, active_screen }, Task::batch([init_task, open_window_task]))
    }

    pub fn subscription(state: &State) -> Subscription<Message> {
        let screen_subscriptions = match &state.active_screen {
            ActiveScreen::Onboarding(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Home(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Call(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Settings(screen) => screen.subscription(&state.ctx),
        };

        let frame_subscription =
            Subscription::run_with(state.ctx.packet_rx.clone(), frame_subscription_stream);

        let event_subscription = if let Some(rx) = &state.ctx.webrtc_event_rx {
            Subscription::run_with(
                WebRTCEventReceiverRef(rx.clone()),
                webrtc_event_subscription_stream,
            )
        } else {
            Subscription::none()
        };

        let window_open_subscription = iced::window::open_events().map(Message::WindowOpened);
        let window_close_subscription = iced::window::close_events().map(Message::WindowClosed);
        let tick_subscription =
            iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);

        Subscription::batch(vec![
            screen_subscriptions,
            frame_subscription,
            event_subscription,
            window_open_subscription,
            window_close_subscription,
            tick_subscription,
        ])
    }

    pub fn update(state: &mut State, message: Message) -> Task<Message> {
        fn delegate_to_screen(state: &mut State, msg: Message) -> Task<Message> {
            let task = match &mut state.active_screen {
                ActiveScreen::Onboarding(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Home(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Call(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Settings(screen) => screen.update(&mut state.ctx, msg),
            };
            task
        }

        fn screen_from_route(
            state: &mut State,
            capture: Arc<RwLock<PlatformCaptureProvider>>,
            route: Route,
        ) -> ActiveScreen {
            match route {
                Route::Home => ActiveScreen::Home(screens::home::HomeScreen::new(&mut state.ctx)),
                Route::Call => ActiveScreen::Call(screens::call::CallScreen::new(capture)),
                Route::Settings => ActiveScreen::Settings(screens::settings::SettingsScreen::new(
                    state.ctx.config.clone(),
                )),
            }
        }

        // Every message should be delegated to the active screen in the case that the active screen also wants to listen to it.
        // The exception being messages like Navigate.
        match message {
            Message::Navigate(route) => {
                state.active_screen = screen_from_route(state, state.ctx.capture.clone(), route);
                Task::none()
            }
            Message::NavigateWithBack(route) => {
                let mut screen = screen_from_route(state, state.ctx.capture.clone(), route);
                std::mem::swap(&mut state.active_screen, &mut screen);
                // screen is not set to the old screen

                state.ctx.back_queue.push_front(screen);
                Task::none()
            }
            Message::Back => {
                if let Some(screen) = state.ctx.back_queue.pop_front() {
                    state.active_screen = screen;
                }
                Task::none()
            }

            Message::Tick(now) => {
                state.ctx.notifications.dismiss_expired(now);
                delegate_to_screen(state, message)
            }
            Message::DismissNotification(id) => {
                state.ctx.notifications.dismiss(id);
                delegate_to_screen(state, message)
            }

            Message::WindowOpened(id) => {
                if state.ctx.main_window.is_none() {
                    state.ctx.main_window = Some(WindowInfo { iced_id: id, raw_id: None });
                }
                Task::batch([
                    iced::window::raw_id::<Message>(id)
                        .map(move |raw_id| Message::WindowRawIdFetched((id, raw_id))),
                    delegate_to_screen(state, message),
                ])
            }
            Message::WindowClosed(id) => {
                if let Some(main_window) = state.ctx.main_window.as_ref()
                    && main_window.iced_id == id
                {
                    state.ctx.main_window = None;
                    return iced::exit();
                }
                delegate_to_screen(state, message)
            }
            Message::WindowRawIdFetched((id, raw_id)) => {
                if let Some(main_window) = state.ctx.main_window.as_mut()
                    && main_window.iced_id == id
                {
                    main_window.raw_id = Some(raw_id);
                }
                delegate_to_screen(state, message)
            }

            Message::PacketReceived(packet) => {
                delegate_to_screen(state, Message::PacketReceived(packet))
            }

            Message::WebRTCInitialized(ref result) => match result.clone() {
                Ok(webrtc) => {
                    tracing::info!("WebRTC state initialized.");
                    state.ctx.notifications.success("Successfully connected to signalling server.");
                    state.ctx.webrtc = Some(webrtc);
                    delegate_to_screen(state, message.clone())
                }

                Err(err) => {
                    let err_msg = format!("Failed to initialize WebRTC: {}", err);
                    tracing::error!(err_msg);
                    state.ctx.notifications.error(err_msg);
                    delegate_to_screen(state, message.clone())
                }
            },

            Message::WebRTCEvent(ref event) => match event {
                WebRTCEvent::IncomingCall(sender) => {
                    tracing::info!("Incoming call from {}", sender);

                    //TODO: be able to accept or reject call

                    state.ctx.target_id = Some(sender.clone());

                    delegate_to_screen(state, message)
                }

                WebRTCEvent::Connected => {
                    tracing::info!("WebRTC Connected!");

                    if let ActiveScreen::Home(_) = state.active_screen {
                        let call_screen = screens::call::CallScreen::new(state.ctx.capture.clone());

                        state.active_screen = ActiveScreen::Call(call_screen);
                    }

                    delegate_to_screen(state, message)
                }

                WebRTCEvent::Disconnected => {
                    tracing::info!("WebRTC Disconnected");
                    delegate_to_screen(state, message)
                }
            },

            msg => delegate_to_screen(state, msg),
        }
    }

    pub fn view<'a>(
        state: &'a State,
        _window: window::Id,
    ) -> Element<'a, Message, iced::Theme, iced::Renderer> {
        let screen_content = match &state.active_screen {
            ActiveScreen::Onboarding(screen) => screen.view(&state.ctx),
            ActiveScreen::Home(screen) => screen.view(&state.ctx),
            ActiveScreen::Call(screen) => screen.view(&state.ctx),
            ActiveScreen::Settings(screen) => screen.view(&state.ctx),
        };

        // Render notifications on a layer above the screen content
        iced::widget::stack![screen_content, state.ctx.notifications.view()].into()
    }
}
