use iced::{Element, Subscription, Task};

use crate::ui::app::{App, Message};

pub mod capture;
pub mod home;

pub trait Screen {
    fn update(
        &self,
        state: &mut <App as iced::Program>::State,
        message: <App as iced::Program>::Message,
    ) -> Task<Message>;
    fn view(&self, state: &<App as iced::Program>::State) -> Element<'_, Message>;
    fn subscription(&self, state: &<App as iced::Program>::State) -> Subscription<Message>;
}
