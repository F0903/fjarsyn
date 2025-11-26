use bytes::Bytes;
use iced::{
    Element, Length, Rectangle, Size, advanced,
    advanced::{
        Widget,
        layout::{self, Layout},
        mouse, renderer,
        widget::Tree,
    },
};

pub struct FrameViewer {
    frame_data: Bytes,
    width: u32,
    height: u32,
}

impl FrameViewer {
    pub fn new(frame_data: Bytes, width: u32, height: u32) -> Self {
        Self { frame_data, width, height }
    }
}

pub fn frame_viewer(frame_data: Bytes, width: u32, height: u32) -> FrameViewer {
    FrameViewer::new(frame_data, width, height)
}

impl<Theme, Message, Renderer> Widget<Message, Theme, Renderer> for FrameViewer
where
    Renderer: iced::advanced::image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fixed(self.width as f32), Length::Fixed(self.height as f32))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.width as f32, self.height as f32))
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
        let img_handle =
            advanced::image::Handle::from_rgba(self.width, self.height, self.frame_data.clone());

        let alloc = match renderer.load_image(&img_handle) {
            Ok(alloc) => alloc,
            Err(err) => {
                eprintln!("Failed to allocate image: {}", err);
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
