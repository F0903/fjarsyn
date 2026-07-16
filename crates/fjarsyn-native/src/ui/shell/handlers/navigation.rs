use iced::Task;

use crate::ui::{
    message::{Message, NavigationMessage},
    screens::ScreenEntry,
    shell::{ActiveScreen, Fjarsyn, ShellContext},
};

/// Navigation changes presentation only. It never connects, disconnects, or
/// changes the active media pipeline.
pub fn handle_navigation_msg(app: &mut Fjarsyn, message: NavigationMessage) -> Task<Message> {
    match navigation_intent(message) {
        NavigationIntent::Replace(route) => {
            app.active_screen = ActiveScreen::from_route(route, ShellContext::new(&app.ctx));
            app.ctx.ui.back_queue.clear();
        }
        NavigationIntent::Push(route) => {
            let current_route = app.active_screen.route();
            let mut next = ActiveScreen::from_route(route, ShellContext::new(&app.ctx));
            std::mem::swap(&mut app.active_screen, &mut next);
            app.ctx.ui.back_queue.push_front(ScreenEntry { route: current_route, screen: next });
        }
        NavigationIntent::Back => {
            if let Some(entry) = app.ctx.ui.back_queue.pop_front() {
                app.active_screen = entry.screen;
            }
        }
    }
    Task::none()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NavigationIntent {
    Replace(crate::ui::message::Route),
    Push(crate::ui::message::Route),
    Back,
}

fn navigation_intent(message: NavigationMessage) -> NavigationIntent {
    match message {
        NavigationMessage::Navigate(route) => NavigationIntent::Replace(route),
        NavigationMessage::NavigateWithBack(route) => NavigationIntent::Push(route),
        NavigationMessage::Back => NavigationIntent::Back,
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_core::peer_session::PeerId;

    use super::*;
    use crate::ui::message::Route;

    #[test]
    fn navigation_reduces_to_presentation_only_intents() {
        let peer = Route::Peer { peer_id: PeerId::new("peer-a").unwrap() };
        assert_eq!(
            navigation_intent(NavigationMessage::Navigate(peer.clone())),
            NavigationIntent::Replace(peer.clone())
        );
        assert_eq!(
            navigation_intent(NavigationMessage::NavigateWithBack(peer.clone())),
            NavigationIntent::Push(peer)
        );
        assert_eq!(navigation_intent(NavigationMessage::Back), NavigationIntent::Back);
        // NavigationIntent intentionally has no connect/disconnect/media
        // variant, so changing routes cannot create a network side effect.
    }
}
