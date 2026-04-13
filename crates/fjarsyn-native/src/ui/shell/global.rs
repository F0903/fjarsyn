use iced::Task;

use super::{Fjarsyn, handlers};
use crate::ui::message::{Message, NotificationMessage};

// Global app orchestration lives here so the Iced entry point can stay focused on
// stitching screen updates together with app-wide side effects.
pub(super) fn handle_global_message(app: &mut Fjarsyn, message: Message) -> Task<Message> {
    match message {
        Message::Navigation(msg) => handlers::navigation::handle_navigation_msg(app, msg),
        Message::Lifecycle(msg) => handlers::lifecycle::handle_lifecycle_msg(app, msg),
        Message::Config(msg) => handlers::config::handle_config_msg(app, msg),
        Message::WindowEvent(msg) => handlers::window::handle_window_event_msg(app, msg),
        Message::WindowControl(msg) => handlers::window::handle_window_control_msg(app, msg),
        Message::ContactData(msg) => handlers::contact::handle_contact_msg(app, msg),
        Message::CallService(msg) => handlers::service::handle_call_service_msg(app, msg),
        Message::Messaging(msg) => handlers::messaging::handle_messaging_msg(app, msg),
        Message::Capture(msg) => handlers::service::handle_capture_msg(app, msg),
        Message::Database(msg) => handlers::service::handle_database_msg(app, msg),
        Message::CallAction(msg) => handlers::call_action::handle_call_action_msg(app, msg),
        Message::Notification(msg) => handle_notification_message(app, msg),
        Message::CopyId(id) => copy_id_task(id),
        Message::Batch(messages) => Task::batch(messages.into_iter().map(|msg| app.update(msg))),
        Message::Tick(now) => handle_tick(app, now),
        _ => Task::none(),
    }
}

fn handle_notification_message(app: &mut Fjarsyn, message: NotificationMessage) -> Task<Message> {
    match message {
        NotificationMessage::DismissNotification(id) => {
            app.ctx.ui.notifications.dismiss(id);
            Task::none()
        }
        NotificationMessage::NotifyError(message) => {
            app.ctx.notify_error(message);
            Task::none()
        }
        NotificationMessage::NotifyInfo(message) => {
            app.ctx.notify_info(message);
            Task::none()
        }
        NotificationMessage::NotifySuccess(message) => {
            app.ctx.notify_success(message);
            Task::none()
        }
    }
}

fn copy_id_task(id: String) -> Task<Message> {
    let copied_id = id.clone();

    Task::batch([
        iced::clipboard::write(copied_id.clone()),
        Task::done(Message::Notification(NotificationMessage::NotifyInfo(format!(
            "Copied ID: {}",
            copied_id
        )))),
    ])
}

fn handle_tick(app: &mut Fjarsyn, now: std::time::Instant) -> Task<Message> {
    if matches!(app.ctx.lifecycle, fjarsyn_core::app::AppLifecycle::ShuttingDown) {
        return Task::none();
    }

    app.ctx.ui.notifications.dismiss_expired(now);

    if app.ctx.session.incoming_call_timeout.is_some_and(|deadline| now >= deadline) {
        app.ctx.notify_info("Missed call.");
        use crate::ui::message::CallActionMessage;
        return Task::done(Message::CallAction(CallActionMessage::DeclineCall));
    }

    Task::none()
}
