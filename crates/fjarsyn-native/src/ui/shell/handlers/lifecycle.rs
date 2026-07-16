use iced::Task;

use crate::ui::{
    message::{LifecycleMessage, Message, RuntimeMessage},
    shell::{AppLifecycle, Fjarsyn},
};

pub fn handle_lifecycle_msg(app: &mut Fjarsyn, message: LifecycleMessage) -> Task<Message> {
    match message {
        LifecycleMessage::RetryStartup => {
            if app.runtime.application.is_some() {
                return Task::none();
            }
            let config = match fjarsyn_core::config::Config::load_or_create() {
                Ok(config) => config,
                Err(error) => {
                    let message = format!("Failed to reload configuration: {error}");
                    app.ctx.lifecycle = AppLifecycle::Failed(message.clone());
                    app.ctx.notify_error(message);
                    return Task::none();
                }
            };
            app.ctx.config = config.clone();
            app.ctx.lifecycle = AppLifecycle::Starting;
            Fjarsyn::start_runtime_task(config, app.runtime.event_tx.clone())
        }
    }
}

pub fn shutdown(app: &mut Fjarsyn) -> Task<Message> {
    if matches!(app.ctx.lifecycle, AppLifecycle::ShuttingDown) {
        return Task::none();
    }
    app.ctx.lifecycle = AppLifecycle::ShuttingDown;
    let Some(runtime) = app.runtime.application.take() else {
        return iced::exit();
    };
    Task::future(async move {
        Message::Runtime(RuntimeMessage::ShutdownFinished(
            runtime.shutdown().await.map_err(std::sync::Arc::new),
        ))
    })
}
