use iced::Task;

use crate::ui::{
    message::{Message, NavigationMessage, Route},
    screens::ScreenEntry,
    shell::{ActiveScreen, Fjarsyn, ShellContext, handlers::messaging},
};

pub fn handle_navigation_msg(app: &mut Fjarsyn, message: NavigationMessage) -> Task<Message> {
    match message {
        NavigationMessage::Navigate(route) => {
            let selected_peer_id = route_selected_peer_id(app, &route);
            let messaging_task = messaging::sync_active_conversation(app, selected_peer_id);
            app.active_screen = ActiveScreen::from_route(
                route,
                ShellContext { state: &app.ctx, runtime: &app.runtime },
            );
            app.ctx.ui.back_queue.clear();
            messaging_task
        }
        NavigationMessage::NavigateWithBack(route) => {
            let selected_peer_id = route_selected_peer_id(app, &route);
            let messaging_task = messaging::sync_active_conversation(app, selected_peer_id);
            let current_route = app
                .active_screen
                .get_route(ShellContext { state: &app.ctx, runtime: &app.runtime });
            let mut next_screen = ActiveScreen::from_route(
                route,
                ShellContext { state: &app.ctx, runtime: &app.runtime },
            );
            std::mem::swap(&mut app.active_screen, &mut next_screen);
            app.ctx
                .ui
                .back_queue
                .push_front(ScreenEntry { route: current_route, screen: next_screen });
            messaging_task
        }
        NavigationMessage::Back => {
            if let Some(entry) = app.ctx.ui.back_queue.pop_front() {
                let selected_peer_id = route_selected_peer_id(app, &entry.route);
                let messaging_task = messaging::sync_active_conversation(app, selected_peer_id);
                app.active_screen = entry.screen;
                messaging_task
            } else {
                Task::none()
            }
        }
    }
}

fn route_selected_peer_id(app: &Fjarsyn, route: &Route) -> Option<String> {
    match route {
        Route::Messages { peer_id } => fjarsyn_core::app::resolve_selected_peer_id(
            &app.ctx.messaging.summaries,
            peer_id.clone(),
        ),
        _ => None,
    }
}
