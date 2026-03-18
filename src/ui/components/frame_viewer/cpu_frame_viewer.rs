use std::sync::Arc;

use iced::{
    Element, Length, Rectangle, Size, advanced,
    advanced::{
        Widget,
        layout::{self, Layout},
        mouse, renderer,
        widget::{Tree, tree},
    },
};
use iced_core;

use crate::media::frame::Frame;

#[derive(Default)]
struct CpuFrameViewerState {
    frame_id: usize,
    image_handle: Option<advanced::image::Handle>,
}

pub struct CpuFrameViewer {
    frame: Arc<Frame>,
}

impl CpuFrameViewer {
    pub fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<Theme, Message, Renderer> Widget<Message, Theme, Renderer> for CpuFrameViewer
where
    Renderer: iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<CpuFrameViewerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(CpuFrameViewerState::default())
    }

    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<CpuFrameViewerState>();
        let frame_id = Arc::as_ptr(&self.frame) as usize;

        if state.frame_id != frame_id {
            state.frame_id = frame_id;
            state.image_handle = if self.frame.format.is_iced_compatible() {
                self.frame.get_software_pixels().map(|pixels| {
                    advanced::image::Handle::from_rgba(
                        self.frame.size.x as u32,
                        self.frame.size.y as u32,
                        pixels,
                    )
                })
            } else {
                None
            };
        }

        let max_size = limits.max();
        let src_width = self.frame.size.x as f32;
        let src_height = self.frame.size.y as f32;

        if src_width == 0.0 || src_height == 0.0 {
            return layout::Node::new(Size::ZERO);
        }

        let scale_x = max_size.width / src_width;
        let scale_y = max_size.height / src_height;
        let scale = scale_x.min(scale_y);

        let size = if scale.is_infinite() {
            Size::new(src_width, src_height)
        } else {
            Size::new(src_width * scale, src_height * scale)
        };

        layout::Node::new(size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<CpuFrameViewerState>();
        let Some(img_handle) = state.image_handle.as_ref() else {
            return;
        };

        let alloc = match renderer.load_image(img_handle) {
            Ok(alloc) => alloc,
            Err(err) => {
                tracing::error!("Failed to allocate image: {}", err);
                return;
            }
        };
        let img = iced_core::Image::new(alloc.handle());
        let bounds = layout.bounds();
        renderer.draw_image(img, bounds, bounds);
    }
}

impl<'a, Message, Theme, Renderer> From<CpuFrameViewer> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
    Message: 'a,
{
    fn from(widget: CpuFrameViewer) -> Self {
        Self::new(widget)
    }
}
