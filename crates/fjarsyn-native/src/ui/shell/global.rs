use iced::Task;

use super::{Fjarsyn, handlers};
use crate::ui::message::{Message, NotificationMessage};

pub(super) fn handle_global_message(app: &mut Fjarsyn, message: Message) -> Task<Message> {
    match message {
        Message::Navigation(message) => handlers::navigation::handle_navigation_msg(app, message),
        Message::Lifecycle(message) => handlers::lifecycle::handle_lifecycle_msg(app, message),
        Message::Config(message) => handlers::config::handle_config_msg(app, message),
        Message::WindowEvent(message) => handlers::window::handle_window_event_msg(app, message),
        Message::WindowControl(message) => {
            handlers::window::handle_window_control_msg(app, message)
        }
        Message::ContactData(message) => handlers::contact::handle_contact_msg(app, message),
        Message::PeerAction(message) => handlers::peer::handle_peer_action(app, message),
        Message::Runtime(message) => handlers::runtime::handle_runtime_msg(app, message),
        Message::Notification(message) => handle_notification_message(app, message),
        Message::CopyId(id) => copy_id_task(id),
        Message::CopyInvite(invite) => copy_invite_task(invite),
        Message::CopyFingerprint(fingerprint) => copy_fingerprint_task(fingerprint),
        Message::Tick(now) => {
            app.ctx.ui.notifications.dismiss_expired(now);
            Task::none()
        }
        _ => Task::none(),
    }
}

fn handle_notification_message(app: &mut Fjarsyn, message: NotificationMessage) -> Task<Message> {
    match message {
        NotificationMessage::DismissNotification(id) => app.ctx.ui.notifications.dismiss(id),
        NotificationMessage::NotifyError(message) => app.ctx.notify_error(message),
        NotificationMessage::NotifyInfo(message) => app.ctx.notify_info(message),
        NotificationMessage::NotifySuccess(message) => app.ctx.notify_success(message),
    }
    Task::none()
}

fn copy_id_task(id: String) -> Task<Message> {
    Task::batch([
        iced::clipboard::write(id.clone()),
        Task::done(Message::Notification(NotificationMessage::NotifyInfo(format!(
            "Copied ID: {id}"
        )))),
    ])
}

fn copy_invite_task(invite: String) -> Task<Message> {
    Task::batch([
        iced::clipboard::write(invite),
        Task::done(Message::Notification(NotificationMessage::NotifyInfo(
            "Copied pairing invite. The other person must import it and compare your full fingerprint.".into(),
        ))),
    ])
}

fn copy_fingerprint_task(fingerprint: String) -> Task<Message> {
    Task::batch([
        iced::clipboard::write(fingerprint),
        Task::done(Message::Notification(NotificationMessage::NotifyInfo(
            "Copied the full identity fingerprint. Copying is only a convenience; compare it over an independent trusted channel before confirming."
                .into(),
        ))),
    ])
}
