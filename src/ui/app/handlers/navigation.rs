use iced::Task;

use crate::ui::{
    app::{ActiveScreen, Fjarsyn},
    message::{Message, NavigationMessage},
};

pub fn handle_navigation_msg(app: &mut Fjarsyn, msg: NavigationMessage) -> Task<Message> {
    match msg {
        NavigationMessage::Navigate(route) => {
            app.active_screen = ActiveScreen::from_route(route, &mut app.ctx);
            app.ctx.ui.back_queue.clear();
            Task::none()
        }
        NavigationMessage::NavigateWithBack(route) => {
            let mut screen = ActiveScreen::from_route(route, &mut app.ctx);
            std::mem::swap(&mut app.active_screen, &mut screen);
            app.ctx.ui.back_queue.push_front(screen);
            Task::none()
        }
        NavigationMessage::Back => {
            if let Some(screen) = app.ctx.ui.back_queue.pop_front() {
                app.active_screen = screen;
            }
            Task::none()
        }
    }
}
