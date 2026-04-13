use std::sync::Arc;

use fjarsyn_core::media::{frame::Frame, gpu_interop, pixel_format::PixelFormat};
use iced::{
    Alignment, Element, Length,
    widget::{Space, container, stack, text},
};

use super::{CallMessage, CallScreen};
use crate::ui::{
    self,
    components::{CpuFrameViewer, GpuFrameViewer},
    message::Message,
    shell::ShellContext,
};

mod controls;
mod local_preview;
mod remote;

const LOCAL_PREVIEW_WIDTH: f32 = 320.0;
const LOCAL_PREVIEW_HEIGHT: f32 = 180.0;
const FLOATING_OVERLAY_PADDING: u16 = 24;
const FLOATING_CARD_PADDING: u16 = 8;

enum CallButtonTone {
    Primary,
    Secondary,
    Danger,
}

struct WaitingStateCopy {
    title: &'static str,
    subtitle: &'static str,
    icon: iced::widget::Text<'static>,
}

struct ControlSpec<'a> {
    label: &'a str,
    icon: iced::widget::Text<'a>,
    action: Option<CallMessage>,
    tone: CallButtonTone,
}

impl CallScreen {
    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let content = stack![
            self.view_remote_video(ctx),
            self.view_local_preview(ctx),
            if ctx.ui.cursor_inside_window {
                container(self.view_controls(ctx))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::End)
                    .padding(FLOATING_OVERLAY_PADDING)
            } else {
                container(Space::new())
            }
        ];

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(ui::theme::main_content_container)
            .into()
    }

    fn preferred_frame_viewer(&self, frame: Arc<Frame>) -> Option<Element<'static, Message>> {
        if self.supports_zero_copy_preview(frame.format, frame.gpu_import_handle().is_some()) {
            Some(GpuFrameViewer::new(frame).into())
        } else if frame.format.supports_software_preview() {
            Some(CpuFrameViewer::new(frame).into())
        } else {
            None
        }
    }

    fn supports_zero_copy_preview(&self, pixel_format: PixelFormat, has_gpu_handle: bool) -> bool {
        has_gpu_handle && gpu_interop::supports_zero_copy_preview(pixel_format)
    }

    fn preview_surface<'a>(&self, content: Element<'a, Message>) -> Element<'a, Message> {
        container(content)
            .width(Length::Fixed(LOCAL_PREVIEW_WIDTH))
            .height(Length::Fixed(LOCAL_PREVIEW_HEIGHT))
            .style(container::bordered_box)
            .into()
    }

    fn view_preview_unavailable(&self) -> Element<'_, Message> {
        self.preview_surface(text("Preview unavailable for this format.").size(12).into())
    }
}
