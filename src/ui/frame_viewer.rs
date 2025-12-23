use std::sync::Arc;

use iced::{
    Element, Length, Rectangle, Size, advanced,
    advanced::{
        Widget,
        layout::{self, Layout},
        mouse, renderer,
        widget::Tree,
    },
};

use crate::capture_providers::shared::Frame;

pub struct FrameViewer {
    frame: Arc<Frame>,
}

impl FrameViewer {
    pub fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<Theme, Message, Renderer> Widget<Message, Theme, Renderer> for FrameViewer
where
    Renderer: iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
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
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let img_handle = advanced::image::Handle::from_rgba(
            self.frame.size.x as u32,
            self.frame.size.y as u32,
            self.frame.data.clone(),
        );

        let alloc = match renderer.load_image(&img_handle) {
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

impl<'a, Message, Theme, Renderer> From<FrameViewer> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
    Message: 'a,
{
    fn from(widget: FrameViewer) -> Self {
        Self::new(widget)
    }
}
