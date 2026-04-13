use std::sync::Arc;

use fjarsyn_core::{
    capture_providers::PlatformCaptureItem,
    media::{frame::Frame, pixel_format::PixelFormat},
};
use futures::stream::unfold;
use iced::{Subscription, Task};

use crate::ui::{
    message::{Message, ScreenMessage},
    screens::Screen,
    shell::{AppContext, AppContextMut},
};

mod capture;
mod handlers;
mod media;
mod state;
mod view;
mod workers;
mod workflow;

use state::{CaptureState, LocalShareState, RemoteVideoState};
use workers::LatestFrameReceiverRef;

type FrameSubscriptionStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Message> + Send + 'static>>;

#[derive(Debug, Clone)]
pub enum CallMessage {
    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(PlatformCaptureItem),
    TryStopCapture,
    PlatformUserPickedCaptureItem(Result<Option<PlatformCaptureItem>, String>),
    LocalFrameReady(Arc<Frame>),
    DecodedFrameReady(Arc<Frame>),
    DecodedFrameCleared,
    RemoteStreamStarted,
    RemoteStreamEnded,
    ToggleLocalPreview,
    EndCall,
}

#[derive(Clone, Debug)]
pub struct CallScreen {
    pub(crate) capture: CaptureState,
    pub(crate) local: LocalShareState,
    pub(crate) remote: RemoteVideoState,
}

impl CallScreen {
    pub fn new(ctx: AppContext<'_>) -> Self {
        Self {
            capture: CaptureState::new(ctx.media.capture.clone()),
            local: LocalShareState::default(),
            remote: RemoteVideoState::new(
                ctx.runtime.frame_packet_rx.clone(),
                ctx.config.video.transcoding_type,
                PixelFormat::DEFAULT_CAPTURE,
            ),
        }
    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.capture.is_capturing()
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum FrameSubscriptionKind {
    Local,
    Remote,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FrameSubscriptionData {
    receiver: LatestFrameReceiverRef,
    kind: FrameSubscriptionKind,
}

fn latest_frame_subscription(data: FrameSubscriptionData) -> Subscription<Message> {
    Subscription::run_with(data, build_latest_frame_subscription)
}

fn build_latest_frame_subscription(data: &FrameSubscriptionData) -> FrameSubscriptionStream {
    let receiver = data.receiver.0.clone();
    let kind = data.kind;

    Box::pin(unfold(receiver, move |receiver| async move {
        loop {
            let result = {
                let mut lock = receiver.lock().await;
                lock.changed().await
            };

            match result {
                Ok(()) => {
                    let frame = {
                        let lock = receiver.lock().await;
                        lock.borrow().clone()
                    };

                    let message = match (kind, frame) {
                        (FrameSubscriptionKind::Local, Some(frame)) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::LocalFrameReady(frame)),
                        )),
                        (FrameSubscriptionKind::Local, None) => None,
                        (FrameSubscriptionKind::Remote, Some(frame)) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::DecodedFrameReady(frame)),
                        )),
                        (FrameSubscriptionKind::Remote, None) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::DecodedFrameCleared),
                        )),
                    };

                    if let Some(message) = message {
                        return Some((message, receiver));
                    }
                }
                Err(_) => return None,
            }
        }
    }))
}

impl Screen for CallScreen {
    fn subscription(&self, _ctx: AppContext<'_>) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if let Some(receiver) = self.local.latest_frame_receiver() {
            subscriptions.push(latest_frame_subscription(FrameSubscriptionData {
                receiver,
                kind: FrameSubscriptionKind::Local,
            }));
        }

        if let Some(receiver) = self.remote.decoded_frame_receiver() {
            subscriptions.push(latest_frame_subscription(FrameSubscriptionData {
                receiver,
                kind: FrameSubscriptionKind::Remote,
            }));
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_update(ctx, message)
    }

    fn view<'a>(&'a self, ctx: AppContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
