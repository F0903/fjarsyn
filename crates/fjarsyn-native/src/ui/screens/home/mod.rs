use iced::{Subscription, Task};

use crate::ui::{
    message::Message,
    screens::Screen,
    shell::{ShellContext, ShellContextMut},
};

mod view;

#[derive(Debug, Clone, Default)]
pub struct HomeScreen;

impl HomeScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: ShellContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut ShellContextMut<'_>, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn view<'a>(&'a self, ctx: ShellContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
