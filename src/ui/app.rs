use std::sync::Arc;

use iced::{Element, Program, Subscription, Task, executor, window};
use tokio::sync::Mutex;

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

    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(PlatformCaptureItem),
    TryStopCapture,

    PlatformUserPickedCaptureItem(Result<PlatformCaptureItem, String>),

    FrameCaptured(Arc<Frame>),
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

pub struct State {
    active_screen: ActiveScreen,
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
        Some(window::Settings::default())
    }

    fn boot(&self) -> (Self::State, Task<Self::Message>) {
        (
            State { active_screen: ActiveScreen::Home(screens::home::HomeScreen::new()) },
            Task::future(async { WebRTC::new().await.map_err(Arc::new) })
                .map(Message::WebRTCInitialized),
        )
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Message> {
        match &state.active_screen {
            ActiveScreen::Home(screen) => screen.subscription(),
            ActiveScreen::Capture(screen) => screen.subscription(),
        }
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Navigate(route) => {
                match route {
                    Route::Home => {
                        state.active_screen = ActiveScreen::Home(screens::home::HomeScreen::new());
                    }
                    Route::Capture => {
                        state.active_screen = ActiveScreen::Capture(
                            screens::capture::CaptureScreen::new(self.capture.clone()),
                        );
                    }
                    Route::Settings => {
                        //TODO
                    }
                }
                Task::none()
            }
            msg => match &mut state.active_screen {
                ActiveScreen::Home(screen) => screen.update(msg),
                ActiveScreen::Capture(screen) => screen.update(msg),
            },
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
