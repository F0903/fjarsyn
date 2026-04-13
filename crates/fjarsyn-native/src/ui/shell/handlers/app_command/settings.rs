use iced::Task;

use crate::ui::{
    message::{ConfigMessage, Message},
    shell::Fjarsyn,
};

pub(super) fn run_save_config(
    app: &mut Fjarsyn,
    success_message: Option<String>,
    error_message: String,
) -> Task<Message> {
    if let Err(err) = app.ctx.config.save() {
        app.ctx.notify_error(format!("{}: {}", error_message, err));
    } else if let Some(message) = success_message {
        app.ctx.notify_success(message);
    }
    Task::none()
}

pub(super) fn run_apply_capture_readback(app: &mut Fjarsyn, enabled: bool) -> Task<Message> {
    let Some(capture) = app.ctx.media.capture.clone() else {
        return Task::none();
    };

    Task::future(async move {
        capture.write().await.set_cpu_readback_enabled(enabled);
        Message::Config(ConfigMessage::CaptureReadbackApplied)
    })
}
