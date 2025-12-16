use std::sync::Arc;

use bytes::Bytes;
use futures::stream::unfold;
use iced::{Element, Program, Subscription, Task, executor, window};
use tokio::sync::{Mutex, mpsc};

use super::screens::{self, Screen};
use crate::{
    capture_providers::{
        PlatformCaptureProvider,
        shared::{CaptureFramerate, Vector2},
    },
    config::Config,
    networking::webrtc::{WebRTC, WebRTCEvent},
    ui::{
        message::{Message, Route},
        state::State,
    },
};

#[derive(Debug, Clone)]
pub enum ActiveScreen {
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

        (
            State {
                active_screen: ActiveScreen::Home(screens::home::HomeScreen::new()),
                main_window_handle: None,

                capturing: false,
                capture_frame_rate: CaptureFramerate::FPS60,

                frame_receiver: FrameReceiverRef(Arc::new(Mutex::new(frame_rx))),
                webrtc_event_receiver: Some(Arc::new(Mutex::new(event_rx))),
                frame_data: None,
                frame_dimensions: Vector2::new(0, 0),
                frame_format: config.pixel_format.clone(),

                webrtc: None,

                target_id: None,

                frame_sender: None,
                decoder: None,
                config,
                pending_config: None,
            },
            Task::future(async move {
                WebRTC::new(server_url, frame_tx, event_tx).await.map_err(Arc::new)
            })
            .map(Message::WebRTCInitialized),
        )
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Message> {
        let screen_subscriptions = match &state.active_screen {
            ActiveScreen::Home(screen) => screen.subscription(state),
            ActiveScreen::Capture(screen) => screen.subscription(state),
            ActiveScreen::Settings(screen) => screen.subscription(state),
        };

        let frame_subscription =
            Subscription::run_with(state.frame_receiver.clone(), frame_subscription_stream);

        let event_subscription = if let Some(rx) = &state.webrtc_event_receiver {
            Subscription::run_with(
                WebRTCEventReceiverRef(rx.clone()),
                webrtc_event_subscription_stream,
            )
        } else {
            Subscription::none()
        };

        let window_open_subscription = iced::window::open_events().map(Message::WindowOpened);

        Subscription::batch(vec![
            screen_subscriptions,
            frame_subscription,
            event_subscription,
            window_open_subscription,
        ])
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        fn delegate_to_screen(state: &mut State, msg: Message) -> Task<Message> {
            match state.active_screen.clone() {
                ActiveScreen::Home(screen) => screen.update(state, msg),
                ActiveScreen::Capture(screen) => screen.update(state, msg),
                ActiveScreen::Settings(screen) => screen.update(state, msg),
            }
        }

        match message {
            Message::WindowOpened(id) => {
                iced::window::raw_id::<Message>(id).map(Message::WindowIdFetched)
            }
            Message::WindowIdFetched(id) => {
                if state.main_window_handle.is_none() {
                    state.main_window_handle = Some(id);
                }
                Task::none()
            }

            Message::Navigate(route) => {
                match route {
                    Route::Home => {
                        state.pending_config = None;
                        state.active_screen = ActiveScreen::Home(screens::home::HomeScreen::new());
                    }
                    Route::Capture => {
                        state.pending_config = None;
                        let capture_screen =
                            screens::capture::CaptureScreen::new(self.capture.clone());
                        state.active_screen = ActiveScreen::Capture(capture_screen);
                    }
                    Route::Settings => {
                        state.pending_config = Some(state.config.clone());
                        state.active_screen =
                            ActiveScreen::Settings(screens::settings::SettingsScreen::new());
                    }
                }
                Task::none()
            }

            Message::SaveConfig => {
                if let Some(pending) = state.pending_config.take() {
                    state.config = pending;
                    if let Err(e) = state.config.save() {
                        tracing::error!("Failed to save config: {}", e);
                    }
                }
                // Navigate back to Home after save
                state.active_screen = ActiveScreen::Home(screens::home::HomeScreen::new());
                Task::none()
            }

            Message::StartCall(target_id) => {
                if let Some(Ok(webrtc)) = &state.webrtc {
                    let webrtc_clone = webrtc.clone();
                    Task::future(async move {
                        match webrtc_clone.create_offer(target_id).await {
                            Ok(_) => Message::Navigate(Route::Capture),
                            Err(e) => Message::Error(format!("Failed to create offer: {}", e)),
                        }
                    })
                } else {
                    tracing::warn!("Could not start call. WebRTC not initialized...");
                    Task::none()
                }
            }

            Message::RemoteFrameReceived(frame) => {
                let mut tasks = vec![];

                // If we receive a frame and we are NOT on Capture screen, switch to it.
                if let ActiveScreen::Home(_) = state.active_screen {
                    // We need to perform the navigation logic here, similar to Message::Navigate
                    let capture_screen = screens::capture::CaptureScreen::new(self.capture.clone());
                    state.active_screen = ActiveScreen::Capture(capture_screen);
                }

                // Delegate the frame to the (now guaranteed) CaptureScreen
                tasks.push(delegate_to_screen(state, Message::RemoteFrameReceived(frame)));
                Task::batch(tasks)
            }

            Message::WebRTCInitialized(result) => {
                match result {
                    Ok(webrtc) => {
                        tracing::info!("WebRTC state initialized.");
                        state.webrtc = Some(Ok(webrtc));
                    }
                    Err(err) => {
                        let err_msg = format!("Failed to initialize WebRTC: {}", err);
                        tracing::error!(err_msg);
                        state.webrtc = Some(Err(err_msg));
                    }
                }
                Task::none()
            }

            Message::WebRTCEvent(event) => match event {
                WebRTCEvent::IncomingCall(sender) => {
                    tracing::info!("Incoming call from {}", sender);
                    state.target_id = Some(sender);
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

            Message::CopyId(id) => iced::clipboard::write(id),

            msg => delegate_to_screen(state, msg),
        }
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        _window: window::Id,
    ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
        match &state.active_screen {
            ActiveScreen::Home(screen) => screen.view(state),
            ActiveScreen::Capture(screen) => screen.view(state),
            ActiveScreen::Settings(screen) => screen.view(state),
        }
    }
}
