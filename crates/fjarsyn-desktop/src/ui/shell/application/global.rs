use iced::Task;

use super::Fjarsyn;
use crate::ui::{
    message::{self, Message},
    shell::handlers,
};

pub(super) fn handle_global_message(app: &mut Fjarsyn, message: Message) -> Task<Message> {
    match message {
        Message::Navigation(message) => handlers::handle_navigation_msg(app, message),
        Message::Lifecycle(message) => handlers::handle_lifecycle_msg(app, message),
        Message::Config(message) => handlers::handle_config_msg(app, message),
        Message::WindowEvent(message) => handlers::handle_window_event_msg(app, message),
        Message::WindowControl(message) => handlers::handle_window_control_msg(app, message),
        Message::ContactOperation(operation) => handlers::handle_contact_operation(app, operation),
        Message::PeerAction(message) => handlers::handle_peer_action(app, message),
        Message::Runtime(message) => handlers::handle_runtime_msg(app, message),
        Message::Notification(message) => handle_notification_message(app, message),
        Message::CopyId(id) => {
            let notice = format!("Copied ID: {id}");
            copy_task(id, notice)
        }
        Message::CopyInvite(invite) => copy_task(
            invite,
            "Copied pairing invite. The other person must import it and compare your full fingerprint.",
        ),
        Message::CopyFingerprint(fingerprint) => copy_task(
            fingerprint,
            "Copied the full identity fingerprint. Copying is only a convenience; compare it over an independent trusted channel before confirming.",
        ),
        Message::Tick(now) => {
            app.state.ui.notifications.dismiss_expired(now);
            Task::none()
        }
        _ => Task::none(),
    }
}

fn handle_notification_message(app: &mut Fjarsyn, message: message::Notification) -> Task<Message> {
    match message {
        message::Notification::Dismiss(id) => app.state.ui.notifications.dismiss(id),
        message::Notification::NotifyError(message) => app.state.notify_error(message),
        message::Notification::NotifyInfo(message) => app.state.notify_info(message),
    }
    Task::none()
}

fn copy_task(value: String, notice: impl Into<String>) -> Task<Message> {
    Task::batch([
        iced::clipboard::write(value),
        Task::done(Message::Notification(message::Notification::NotifyInfo(notice.into()))),
    ])
}
