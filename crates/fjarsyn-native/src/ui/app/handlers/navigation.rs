use iced::Task;

use crate::ui::{
    app::{ActiveScreen, AppContext, Fjarsyn, handlers::messaging},
    message::{Message, NavigationMessage, Route},
    screens::messages,
};

pub fn handle_navigation_msg(app: &mut Fjarsyn, message: NavigationMessage) -> Task<Message> {
    match message {
        NavigationMessage::Navigate(route) => {
            let selected_peer_id = route_selected_peer_id(app, &route);
            app.active_screen = ActiveScreen::from_route(
                route,
                AppContext { state: &app.ctx, runtime: &app.runtime },
            );
            app.ctx.ui.back_queue.clear();
            messaging::sync_active_conversation(app, selected_peer_id)
        }
        NavigationMessage::NavigateWithBack(route) => {
            let selected_peer_id = route_selected_peer_id(app, &route);
            let mut next_screen = ActiveScreen::from_route(
                route,
                AppContext { state: &app.ctx, runtime: &app.runtime },
            );
            std::mem::swap(&mut app.active_screen, &mut next_screen);
            app.ctx.ui.back_queue.push_front(next_screen);
            messaging::sync_active_conversation(app, selected_peer_id)
        }
        NavigationMessage::Back => {
            if let Some(screen) = app.ctx.ui.back_queue.pop_front() {
                app.active_screen = screen;
                let selected_peer_id = match &app.active_screen {
                    ActiveScreen::Messages(screen) => screen.selected_peer_id.clone(),
                    _ => None,
                };
                messaging::sync_active_conversation(app, selected_peer_id)
            } else {
                Task::none()
            }
        }
    }
}

fn route_selected_peer_id(app: &Fjarsyn, route: &Route) -> Option<String> {
    match route {
        Route::Messages { peer_id } => messages::resolve_selected_peer_id(
            AppContext { state: &app.ctx, runtime: &app.runtime },
            peer_id.clone(),
        ),
        _ => None,
    }
}
