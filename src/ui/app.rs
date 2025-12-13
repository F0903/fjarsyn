use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use futures::stream::unfold;
use iced::{Element, Program, Subscription, Task, executor, window};
use tokio::sync::{Mutex, mpsc};

use super::screens::{self, Screen};
use crate::{
    capture_providers::{
        PlatformCaptureItem, PlatformCaptureProvider,
        shared::{CaptureFramerate, Frame},
    },
    networking::webrtc::{WebRTC, WebRTCError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home, //TODO
    Capture,
    Settings, //TODO
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),

    StartCall(String),
    RemoteIdChanged(String),
    LocalIdFetched(String),

    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(PlatformCaptureItem),
    TryStopCapture,

    PlatformUserPickedCaptureItem(Result<PlatformCaptureItem, String>),

    FrameCaptured(Arc<Frame>),
    RemoteFrameReceived(Bytes),
    FrameRateSelected(CaptureFramerate),

    WebRTCInitialized(Result<WebRTC, Arc<WebRTCError>>),

    WindowOpened(window::Id),
    WindowIdFetched(u64),

    Error(String),
    NoOp,
}

pub enum ActiveScreen {
    Home(screens::home::HomeScreen),
    Capture(screens::capture::CaptureScreen),
}

#[derive(Clone)]
struct FrameReceiverRef(Arc<Mutex<mpsc::Receiver<Bytes>>>);

impl Hash for FrameReceiverRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl PartialEq for FrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FrameReceiverRef {}

pub struct State {
    active_screen: ActiveScreen,
    webrtc: Option<WebRTC>,
    frame_receiver: FrameReceiverRef,
}

pub struct App {
    capture: Arc<Mutex<PlatformCaptureProvider>>,
}

impl App {
    const APP_TITLE: &'static str = "loki";

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
            // Channel closed
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
        let (frame_tx, frame_rx) = mpsc::channel(REMOTE_FRAMES_BUFFER);
        (
            State {
                active_screen: ActiveScreen::Home(screens::home::HomeScreen::new()),
                webrtc: None,
                frame_receiver: FrameReceiverRef(Arc::new(Mutex::new(frame_rx))),
            },
            Task::future(async { WebRTC::new(frame_tx).await.map_err(Arc::new) })
                .map(Message::WebRTCInitialized),
        )
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Message> {
        let screen_subscriptions = match &state.active_screen {
            ActiveScreen::Home(screen) => screen.subscription(),
            ActiveScreen::Capture(screen) => screen.subscription(),
        };

        let frame_subscription =
            Subscription::run_with(state.frame_receiver.clone(), frame_subscription_stream);

        Subscription::batch(vec![screen_subscriptions, frame_subscription])
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        fn delegate_to_screen(state: &mut State, msg: Message) -> Task<Message> {
            match &mut state.active_screen {
                ActiveScreen::Home(screen) => screen.update(msg),
                ActiveScreen::Capture(screen) => screen.update(msg),
            }
        }

        match message {
            Message::StartCall(target_id) => {
                if let Some(webrtc) = &state.webrtc {
                    let webrtc_clone = webrtc.clone();
                    Task::future(async move {
                        match webrtc_clone.create_offer(target_id).await {
                            Ok(_) => Message::Navigate(Route::Capture),
                            Err(e) => Message::Error(format!("Failed to create offer: {}", e)),
                        }
                    })
                } else {
                    Task::none()
                }
            }

            Message::RemoteFrameReceived(frame) => {
                // If we receive a frame and we are NOT on Capture screen, switch to it.
                // This handles the "Receiver" side automatically.
                let mut tasks = vec![];

                if let ActiveScreen::Home(_) = state.active_screen {
                    // We need to perform the navigation logic here, similar to Message::Navigate
                    let mut capture_screen =
                        screens::capture::CaptureScreen::new(self.capture.clone());
                    if let Some(webrtc) = &state.webrtc {
                        capture_screen.webrtc = Some(webrtc.clone());
                    }
                    state.active_screen = ActiveScreen::Capture(capture_screen);
                }

                // Delegate the frame to the (now guaranteed) CaptureScreen
                tasks.push(delegate_to_screen(state, Message::RemoteFrameReceived(frame)));
                Task::batch(tasks)
            }

            Message::Navigate(route) => {
                match route {
                    Route::Home => {
                        state.active_screen = ActiveScreen::Home(screens::home::HomeScreen::new());
                    }
                    Route::Capture => {
                        let mut capture_screen =
                            screens::capture::CaptureScreen::new(self.capture.clone());
                        if let Some(webrtc) = &state.webrtc {
                            capture_screen.webrtc = Some(webrtc.clone());
                        }
                        state.active_screen = ActiveScreen::Capture(capture_screen);
                    }
                    Route::Settings => {
                        //TODO
                    }
                }
                Task::none()
            }

            Message::WebRTCInitialized(result) => {
                match result {
                    Ok(webrtc) => {
                        tracing::info!("WebRTC state initialized.");
                        state.webrtc = Some(webrtc.clone());

                        // If we are already on Capture screen, inject it
                        if let ActiveScreen::Capture(screen) = &mut state.active_screen {
                            screen.webrtc = Some(webrtc);
                        }
                    }
                    Err(err) => {
                        tracing::error!("Failed to initialize WebRTC: {}", err);
                    }
                }
                Task::none()
            }

            msg => delegate_to_screen(state, msg),
        }
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        _window: window::Id,
    ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
        match &state.active_screen {
            ActiveScreen::Home(screen) => screen.view(),
            ActiveScreen::Capture(screen) => screen.view(),
        }
    }
}
