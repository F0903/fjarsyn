use std::sync::Arc;

use fjarsyn_core::{
    capture_providers::PlatformCaptureItem,
    media::{frame::Frame, pixel_format::PixelFormat},
};
use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{ShellContext, ShellContextMut},
};

mod capture;
mod handlers;
mod media;
mod pipeline;
mod state;
mod subscription;
mod view;
mod workflow;

use state::{CaptureState, LocalShareState, RemoteVideoState};

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
    pub fn new(ctx: ShellContext<'_>) -> Self {
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

impl Screen for CallScreen {
    fn subscription(&self, _ctx: ShellContext<'_>) -> Subscription<Message> {
        subscription::build(self)
    }

    fn update(&mut self, ctx: &mut ShellContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_update(ctx, message)
    }

    fn view<'a>(&'a self, ctx: ShellContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
