use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use futures::stream::unfold;
use iced::{Subscription, Task};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::{
        CaptureFramerate, CaptureProvider, PlatformCaptureProvider, PlatformCaptureStream,
    },
    media::frame::Frame,
    ui::{
        app::AppState,
        message::{Message, ScreenMessage},
        screens::Screen,
    },
};

mod handlers;
mod view;

#[derive(Debug, Clone)]
pub struct FrameReceiverSubData {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,
    pub(crate) framerate: CaptureFramerate,
    pub(crate) revision: u64,
}

impl Hash for FrameReceiverSubData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.capture) as *const ()).hash(state);
        self.framerate.hash(state);
        self.revision.hash(state);
    }
}

impl PartialEq for FrameReceiverSubData {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.capture, &other.capture)
            && self.framerate == other.framerate
            && self.revision == other.revision
    }
}

impl Eq for FrameReceiverSubData {}

#[derive(Clone)]
pub struct DecodedFrameReceiverRef(pub Arc<Mutex<mpsc::UnboundedReceiver<Arc<Frame>>>>);

impl std::fmt::Debug for DecodedFrameReceiverRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DecodedFrameReceiverRef")
    }
}

impl Hash for DecodedFrameReceiverRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl PartialEq for DecodedFrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for DecodedFrameReceiverRef {}

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
    pub(crate) capture: Option<Arc<RwLock<PlatformCaptureProvider>>>,

    // Local Capture State
    pub(crate) local_frame: Option<Arc<Frame>>,
    pub(crate) frame_sender: Option<mpsc::Sender<Arc<Frame>>>,
    pub(crate) capture_subscription_revision: u64,
    pub(crate) pending_capture_start: bool,
    pub(crate) show_local_preview: bool,

    // Remote Capture State
    pub(crate) remote_frame: Option<Arc<Frame>>,
    pub(crate) decoder_sender: Option<mpsc::UnboundedSender<bytes::Bytes>>,
    pub(crate) decoded_frame_rx: Option<DecodedFrameReceiverRef>,
}

impl CallScreen {
    pub fn new(capture: Option<Arc<RwLock<PlatformCaptureProvider>>>) -> Self {
        Self {
            capture,

            local_frame: None,
            frame_sender: None,
            capture_subscription_revision: 0,
            pending_capture_start: false,
            show_local_preview: false,

            remote_frame: None,
            decoder_sender: None,
            decoded_frame_rx: None,
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
        self.capture
            .as_ref()
            .and_then(|capture| capture.try_read().ok())
            .map(|capture| capture.is_capturing())
            .unwrap_or(false)
    }
}

impl Screen for CallScreen {
    fn subscription(&self, ctx: &AppState) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if self.is_capturing()
            && let Some(capture) = self.capture.clone()
        {
            subscriptions.push(
                Subscription::<Frame>::run_with(
                    FrameReceiverSubData {
                        capture,
                        framerate: ctx.config.target_framerate,
                        revision: self.capture_subscription_revision,
                    },
                    Self::create_frame_receiver_subscription,
                )
                .map(|f| {
                    Message::Screen(ScreenMessage::Call(CallMessage::FrameCaptured(Arc::new(f))))
                }),
            );
        }

        if let Some(receiver) = self.decoded_frame_rx.clone() {
            subscriptions.push(Subscription::run_with(receiver, |receiver_ref| {
                let receiver = receiver_ref.0.clone();
                Box::new(Box::pin(unfold(receiver, |receiver| async move {
                    let mut lock = receiver.lock().await;
                    if let Some(frame) = lock.recv().await {
                        drop(lock);
                        Some((
                            Message::Screen(ScreenMessage::Call(CallMessage::DecodedFrameReady(
                                frame,
                            ))),
                            receiver,
                        ))
                    } else {
                        drop(lock);
                        None
                    }
                })))
            }));
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, ctx: &mut AppState, message: Message) -> Task<Message> {
        self.handle_update(ctx, message)
    }

    fn view<'a>(&'a self, ctx: &'a AppState) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
