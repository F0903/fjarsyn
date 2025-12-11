use iced::{Element, Subscription, Task};

use crate::ui::app::Message;

pub mod capture;
pub mod home;

pub trait Screen {
    fn update(&mut self, message: Message) -> Task<Message>;
    fn view(&self) -> Element<'_, Message>;
    fn subscription(&self) -> Subscription<Message>;
}
