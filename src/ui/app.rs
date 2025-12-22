use std::sync::Arc;

use bytes::Bytes;
use futures::stream::unfold;
use iced::{Element, Program, Subscription, Task, executor, window};
use tokio::sync::{Mutex, mpsc};

use super::screens::{self, Screen};
use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::{
        message::{Message, Route},
        notification_provider::NotificationProvider,
        state::{AppContext, State},
    },
};

#[derive(Debug, Clone)]
pub enum ActiveScreen {
    Onboarding(screens::onboarding::OnboardingScreen),
    Home(screens::home::HomeScreen),
    Capture(screens::capture::CaptureScreen),
    Settings(screens::settings::SettingsScreen),
}

pub struct App {
    capture: Arc<Mutex<PlatformCaptureProvider>>,
}

impl App {
    const APP_TITLE: &'static str = "Fjarsyn";

    pub fn new(
        capture: Arc<Mutex<PlatformCaptureProvider>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { capture })
    }

    pub fn run(self) -> crate::Result<()> {
        iced_winit::run(self)?;
        Ok(())
    }
}

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
pub struct FrameReceiverRef(pub Arc<Mutex<mpsc::Receiver<Bytes>>>);

impl std::hash::Hash for FrameReceiverRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl PartialEq for FrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FrameReceiverRef {}

fn frame_subscription_stream(
    receiver_ref: &FrameReceiverRef,
) -> Box<dyn futures::Stream<Item = Message> + Send + Unpin> {
    let receiver = receiver_ref.0.clone();
    Box::new(Box::pin(unfold(receiver, |receiver| async move {
        let mut lock = receiver.lock().await;
        if let Some(frame) = lock.recv().await {
            drop(lock);
            Some((Message::RemoteFrameReceived(frame), receiver))
        } else {
            drop(lock);
            None
        }
    })))
}

impl Program for App {
    type State = State;
    type Message = Message;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;
    type Executor = executor::Default;

    fn name() -> &'static str {
        Self::APP_TITLE
    }

    fn settings(&self) -> iced::Settings {
        iced::Settings::default()
    }

    fn window(&self) -> Option<window::Settings> {
        Some(window::Settings { visible: true, transparent: true, ..Default::default() })
    }

    fn boot(&self) -> (Self::State, Task<Self::Message>) {
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
            main_window_handle: None,

            frame_tx: Some(frame_tx),
            frame_rx: FrameReceiverRef(Arc::new(Mutex::new(frame_rx))),
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
            Task::future(async move { WebRTC::new(server_url, init_frame_tx, init_event_tx).await })
                .map_err(Arc::new)
                .map(Message::WebRTCInitialized)
        } else {
            Task::none()
        };

        (State { ctx, active_screen }, init_task)
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Message> {
        let screen_subscriptions = match &state.active_screen {
            ActiveScreen::Onboarding(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Home(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Capture(screen) => screen.subscription(&state.ctx),
            ActiveScreen::Settings(screen) => screen.subscription(&state.ctx),
        };

        let frame_subscription =
            Subscription::run_with(state.ctx.frame_rx.clone(), frame_subscription_stream);

        let event_subscription = if let Some(rx) = &state.ctx.webrtc_event_rx {
            Subscription::run_with(
                WebRTCEventReceiverRef(rx.clone()),
                webrtc_event_subscription_stream,
            )
        } else {
            Subscription::none()
        };

        let window_open_subscription = iced::window::open_events().map(Message::WindowOpened);
        let tick_subscription =
            iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);

        Subscription::batch(vec![
            screen_subscriptions,
            frame_subscription,
            event_subscription,
            window_open_subscription,
            tick_subscription,
        ])
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        fn delegate_to_screen(state: &mut State, msg: Message) -> Task<Message> {
            let task = match &mut state.active_screen {
                ActiveScreen::Onboarding(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Home(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Capture(screen) => screen.update(&mut state.ctx, msg),
                ActiveScreen::Settings(screen) => screen.update(&mut state.ctx, msg),
            };
            task
        }

        match message {
            Message::Tick(now) => {
                state.ctx.notifications.dismiss_expired(now);
                Task::none()
            }
            Message::DismissNotification(id) => {
                state.ctx.notifications.dismiss(id);
                Task::none()
            }
            Message::WindowOpened(id) => {
                iced::window::raw_id::<Message>(id).map(Message::WindowIdFetched)
            }

            Message::WindowIdFetched(id) => {
                if state.ctx.main_window_handle.is_none() {
                    state.ctx.main_window_handle = Some(id);
                }

                Task::none()
            }

            Message::Navigate(route) => match route {
                Route::Home => {
                    state.active_screen =
                        ActiveScreen::Home(screens::home::HomeScreen::new(&mut state.ctx));
                    Task::none()
                }

                Route::Capture => {
                    let capture_screen = screens::capture::CaptureScreen::new(self.capture.clone());
                    state.active_screen = ActiveScreen::Capture(capture_screen);
                    Task::none()
                }

                Route::Settings => {
                    state.active_screen = ActiveScreen::Settings(
                        screens::settings::SettingsScreen::new(state.ctx.config.clone()),
                    );
                    Task::none()
                }
            },

            // Sub-messages are delegated at the end, but specific global ones like WebRTCEvent remain here.
            Message::RemoteFrameReceived(frame) => {
                let mut tasks = vec![];

                // If we receive a frame and we are NOT on Capture screen, switch to it.
                let should_switch =
                    if let ActiveScreen::Home(_) = state.active_screen { true } else { false };

                if should_switch {
                    let capture_screen = screens::capture::CaptureScreen::new(self.capture.clone());
                    state.active_screen = ActiveScreen::Capture(capture_screen);
                }

                // Delegate the frame to the (now guaranteed) CaptureScreen
                tasks.push(delegate_to_screen(state, Message::RemoteFrameReceived(frame)));
                Task::batch(tasks)
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

            Message::WebRTCEvent(event) => match event {
                WebRTCEvent::IncomingCall(sender) => {
                    tracing::info!("Incoming call from {}", sender);

                    state.ctx.target_id = Some(sender);

                    Task::none()
                }

                WebRTCEvent::Connected => {
                    tracing::info!("WebRTC Connected!");

                    if let ActiveScreen::Home(_) = state.active_screen {
                        let capture_screen =
                            screens::capture::CaptureScreen::new(self.capture.clone());

                        state.active_screen = ActiveScreen::Capture(capture_screen);
                    }

                    Task::none()
                }

                WebRTCEvent::Disconnected => {
                    tracing::info!("WebRTC Disconnected");

                    Task::none()
                }
            },

            msg => delegate_to_screen(state, msg),
        }
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        _window: window::Id,
    ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
        let screen_content = match &state.active_screen {
            ActiveScreen::Onboarding(screen) => screen.view(&state.ctx),
            ActiveScreen::Home(screen) => screen.view(&state.ctx),
            ActiveScreen::Capture(screen) => screen.view(&state.ctx),
            ActiveScreen::Settings(screen) => screen.view(&state.ctx),
        };

        // Render notifications on a layer above the screen content
        iced::widget::stack![screen_content, state.ctx.notifications.view()].into()
    }
}
