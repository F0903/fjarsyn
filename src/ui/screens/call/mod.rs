use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use iced::{Subscription, Task};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::{
        CaptureFramerate, CaptureProvider, PlatformCaptureProvider, PlatformCaptureStream,
    },
    media::ffmpeg::FFmpegDecoder,
    ui::{
        app::AppContext,
        message::{Message, ScreenMessage},
        screens::Screen,
    },
    utils::frame::Frame,
};

mod handlers;
mod view;

#[derive(Debug, Clone)]
pub struct FrameReceiverSubData {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,
    pub(crate) framerate: CaptureFramerate,
    pub(crate) stream_name: &'static str,
}

impl Hash for FrameReceiverSubData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.stream_name.hash(state);
    }
}

#[derive(Debug, Clone)]
pub enum CallMessage {
    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(crate::capture_providers::PlatformCaptureItem),
    TryStopCapture,
    PlatformUserPickedCaptureItem(Result<crate::capture_providers::PlatformCaptureItem, String>),
    FrameCaptured(Arc<Frame>),
    DecodedFrameReady(Arc<Frame>),
    ToggleLocalPreview,
    EndCall,
}

#[derive(Clone, Debug)]
pub struct CallScreen {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,

    // Local Capture State
    pub(crate) local_frame: Option<Arc<Frame>>,
    pub(crate) frame_sender: Option<mpsc::Sender<Arc<Frame>>>,
    pub(crate) show_local_preview: bool,

    // Remote Capture State
    pub(crate) remote_frame: Option<Arc<Frame>>,
    pub(crate) decoder: Option<Arc<Mutex<FFmpegDecoder>>>,
}

impl CallScreen {
    pub fn new(capture: Arc<RwLock<PlatformCaptureProvider>>) -> Self {
        Self {
            capture,

            local_frame: None,
            frame_sender: None,
            show_local_preview: false,

            remote_frame: None,
            decoder: None,
        }
    }

    pub(crate) fn create_frame_receiver_subscription(
        data: &FrameReceiverSubData,
    ) -> PlatformCaptureStream {
        tracing::info!("Creating frame receiver sub with framerate: {}", data.framerate);

        data.capture
            .blocking_write()
            .create_stream(data.framerate)
            .expect("Failed to create stream!")
    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.capture.try_read().map(|c| c.is_capturing()).unwrap_or(false)
    }
}

impl Screen for CallScreen {
    fn subscription(&self, ctx: &AppContext) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if self.is_capturing() {
            subscriptions.push(
                Subscription::<Frame>::run_with(
                    FrameReceiverSubData {
                        capture: self.capture.clone(),
                        framerate: ctx.config.target_framerate,
                        stream_name: "frame-receiver",
                    },
                    Self::create_frame_receiver_subscription,
                )
                .map(|f| {
                    Message::Screen(ScreenMessage::Call(CallMessage::FrameCaptured(Arc::new(f))))
                }),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        self.handle_update(ctx, message)
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
