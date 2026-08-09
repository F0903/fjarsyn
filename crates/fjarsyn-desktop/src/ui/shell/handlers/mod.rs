//! Shell-owned application effects dispatched by the exhaustive root update loop.

use iced::Task;

use crate::ui::{
    message::{self, Message},
    screens::Active,
    shell::{Fjarsyn, Lifecycle},
};

mod contact;
mod lifecycle;
mod peer;
mod runtime;
mod settings;
mod window;

pub(super) use contact::handle_contact_operation;
pub(super) use lifecycle::{handle_lifecycle_msg, shutdown};
pub(super) use peer::handle_peer_action;
pub(super) use runtime::handle_runtime_msg;
pub(super) use settings::handle_settings_msg;
pub(super) use window::{handle_window_control_msg, handle_window_event_msg};

/// Navigation changes presentation only. It never connects, disconnects, or
/// changes the active media pipeline.
pub(super) fn handle_navigation_msg(
    app: &mut Fjarsyn,
    message: message::Navigation,
) -> Task<Message> {
    let message::Navigation::Navigate(route) = message;
    if !accepts_navigation(&app.state.lifecycle, &route) {
        return Task::none();
    }
    app.active_screen = Active::from_route(route, app.state.presentation());
    Task::none()
}

fn accepts_navigation(lifecycle: &Lifecycle, route: &message::Route) -> bool {
    matches!(lifecycle, Lifecycle::Ready)
        || matches!(lifecycle, Lifecycle::StartupFailed(_))
            && matches!(route, message::Route::Home | message::Route::Settings)
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::identity::PeerId;

    use super::{Lifecycle, accepts_navigation};
    use crate::ui::message::Route;

    #[test]
    fn startup_failure_navigation_is_limited_to_recovery_views() {
        let failed = Lifecycle::StartupFailed("failed".into());

        assert!(accepts_navigation(&failed, &Route::Home));
        assert!(accepts_navigation(&failed, &Route::Settings));
        assert!(!accepts_navigation(
            &failed,
            &Route::Peer { peer_id: PeerId::new("peer").unwrap() },
        ));
        assert!(!accepts_navigation(&Lifecycle::Starting, &Route::Settings));
        assert!(accepts_navigation(&Lifecycle::Ready, &Route::Contacts));
    }
}
