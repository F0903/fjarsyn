use iced::Task;

use crate::ui::{
    app::{Fjarsyn, workflows::navigation},
    message::{Message, NavigationMessage},
};

pub fn handle_navigation_msg(app: &mut Fjarsyn, message: NavigationMessage) -> Task<Message> {
    navigation::reduce(app, message);
    Task::none()
}
