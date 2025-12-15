use std::sync::Arc;

use bytes::Bytes;
use futures::stream::unfold;
use iced::{Element, Program, Subscription, Task, executor, window};
use tokio::sync::{Mutex, mpsc};

use super::screens::{self, Screen};
use crate::{
    capture_providers::{
        PlatformCaptureItem, PlatformCaptureProvider, TARGET_PIXEL_FORMAT,
        shared::{CaptureFramerate, Frame, Vector2},
    },
    networking::webrtc::{WebRTC, WebRTCError},
    ui::{frame_receiver_ref::FrameReceiverRef, state::State},
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
    StartCall(String),

    TargetIdChanged(String),

    WindowOpened(window::Id),
    WindowIdFetched(u64),

    Error(String),
    NoOp,
}

#[derive(Debug, Clone)]
pub enum ActiveScreen {
    Home(screens::home::HomeScreen),
    Capture(screens::capture::CaptureScreen),
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
                active_window_handle: None,

                capturing: false,
                capture_frame_rate: CaptureFramerate::FPS60,

                frame_receiver: FrameReceiverRef(Arc::new(Mutex::new(frame_rx))),
                frame_data: None,
                frame_dimensions: Vector2::new(0, 0),
                frame_format: TARGET_PIXEL_FORMAT,

                webrtc: None,

                target_id: None,

                encoder: None,
                decoder: None,
            },
            Task::future(async { WebRTC::new(frame_tx).await.map_err(Arc::new) })
                .map(Message::WebRTCInitialized),
        )
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Message> {
        let screen_subscriptions = match &state.active_screen {
            ActiveScreen::Home(screen) => screen.subscription(state),
            ActiveScreen::Capture(screen) => screen.subscription(state),
        };

        let frame_subscription =
            Subscription::run_with(state.frame_receiver.clone(), frame_subscription_stream);

        Subscription::batch(vec![screen_subscriptions, frame_subscription])
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        fn delegate_to_screen(state: &mut State, msg: Message) -> Task<Message> {
            match state.active_screen.clone() {
                ActiveScreen::Home(screen) => screen.update(state, msg),
                ActiveScreen::Capture(screen) => screen.update(state, msg),
            }
        }

        match message {
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

            Message::Navigate(route) => {
                match route {
                    Route::Home => {
                        state.active_screen = ActiveScreen::Home(screens::home::HomeScreen::new());
                    }
                    Route::Capture => {
                        let capture_screen =
                            screens::capture::CaptureScreen::new(self.capture.clone());
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
        }
    }
}
