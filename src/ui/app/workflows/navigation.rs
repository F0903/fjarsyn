use crate::ui::{
    app::{ActiveScreen, Fjarsyn},
    message::NavigationMessage,
};

// Navigation has no async/runtime work of its own. The workflow exists to keep
// screen transition decisions out of the Iced-facing handler layer.
pub(crate) fn reduce(app: &mut Fjarsyn, message: NavigationMessage) {
    match message {
        NavigationMessage::Navigate(route) => {
            app.active_screen = ActiveScreen::from_route(route, &mut app.ctx);
            app.ctx.ui.back_queue.clear();
        }
        NavigationMessage::NavigateWithBack(route) => {
            let mut next_screen = ActiveScreen::from_route(route, &mut app.ctx);
            std::mem::swap(&mut app.active_screen, &mut next_screen);
            app.ctx.ui.back_queue.push_front(next_screen);
        }
        NavigationMessage::Back => {
            if let Some(screen) = app.ctx.ui.back_queue.pop_front() {
                app.active_screen = screen;
            }
        }
    }
}
