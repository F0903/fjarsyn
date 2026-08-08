use iced::Task;

use crate::ui::{message, presentation::Context};

#[derive(Debug, Clone, Default)]
pub(in crate::ui::screens) struct Screen;

impl Screen {
    pub(in crate::ui::screens) fn new() -> Self {
        Self
    }
}

impl crate::ui::screens::Screen for Screen {
    fn update(
        &mut self,
        _context: Context<'_>,
        _message: message::Message,
    ) -> Task<message::Message> {
        Task::none()
    }

    fn view<'a>(&'a self, context: Context<'a>) -> iced::Element<'a, message::Message> {
        self.render_view(context)
    }
}
