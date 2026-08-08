use std::sync::Arc;

use iced::Task;

use super::event;
use crate::ui::{
    message::{self, Message},
    shell::{Fjarsyn, Lifecycle, relaunch},
};

pub(in crate::ui::shell) fn handle_runtime_msg(
    app: &mut Fjarsyn,
    message: message::Runtime,
) -> Task<Message> {
    match message {
        message::Runtime::Initialized(result) => initialize(app, result),
        message::Runtime::Event(runtime_event) => {
            event::apply(app, runtime_event);
            Task::none()
        }
        message::Runtime::ShutdownFinished(result) => finish_shutdown(result),
        message::Runtime::RestartFinished { shutdown_warning, launch_result } => {
            finish_restart(app, shutdown_warning, launch_result)
        }
    }
}

fn initialize(
    app: &mut Fjarsyn,
    result: Result<crate::ui::runtime::Slot, Arc<String>>,
) -> Task<Message> {
    match result {
        Ok(slot) => {
            let Some(runtime) = slot.take() else {
                return Task::none();
            };
            app.state.config = runtime.active_config().clone();
            app.state.local_peer_id = Some(runtime.local_peer_id().clone());
            app.state.local_public_key = Some(runtime.local_public_key().to_owned());
            app.state.contact_projection = Some(runtime.contacts().projection());
            app.state.presence = runtime.presence().snapshot();
            app.state.sessions = runtime.sessions().snapshot();
            app.state.messaging = runtime.messaging().snapshot();
            app.runtime.application = Some(runtime);
            app.state.lifecycle = Lifecycle::Ready;
        }
        Err(error) => {
            app.state.lifecycle = Lifecycle::Failed(error.to_string());
        }
    }
    Task::none()
}

fn finish_shutdown(result: Result<(), Arc<String>>) -> Task<Message> {
    if let Err(error) = result {
        tracing::warn!("application shutdown completed with errors: {error}");
    }
    iced::exit()
}

fn finish_restart(
    app: &mut Fjarsyn,
    shutdown_warning: Option<Arc<String>>,
    launch_result: Result<(), Arc<String>>,
) -> Task<Message> {
    let Some(should_exit) = relaunch::finish(&mut app.state.lifecycle, &launch_result) else {
        return Task::none();
    };
    if let Some(error) = shutdown_warning {
        tracing::warn!("application shutdown completed with errors before restart: {error}");
    }
    if should_exit { iced::exit() } else { Task::none() }
}
