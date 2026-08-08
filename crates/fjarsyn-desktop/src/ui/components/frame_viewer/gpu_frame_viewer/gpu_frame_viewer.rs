use std::sync::Arc;

use fjarsyn_engine::media::frame::Frame;
use iced::{Element, Length, Rectangle, mouse, widget::shader};
use iced_wgpu::graphics::Viewport;

use super::Pipeline;

pub(in crate::ui) struct GpuFrameViewer {
    frame: Arc<Frame>,
}

impl GpuFrameViewer {
    pub(in crate::ui) fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<'a, Message: 'a> From<GpuFrameViewer> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(viewer: GpuFrameViewer) -> Self {
        shader::Shader::new(Program::new(viewer.frame))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

struct Program {
    frame: Arc<Frame>,
}

impl Program {
    fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<Message> shader::Program<Message> for Program {
    type State = ();
    type Primitive = Primitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        Primitive::new(self.frame.clone())
    }
}

#[derive(Debug)]
struct Primitive {
    frame: Arc<Frame>,
}

impl Primitive {
    fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        pipeline.prepare_frame(device, queue, bounds, &self.frame);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.draw_frame(render_pass, &self.frame)
    }
}
