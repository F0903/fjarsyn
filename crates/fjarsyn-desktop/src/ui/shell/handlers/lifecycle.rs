use iced::Task;

use crate::ui::{
    message::{self, Message},
    shell::{Fjarsyn, Lifecycle, relaunch},
};

pub(in crate::ui::shell) fn handle_lifecycle_msg(
    app: &mut Fjarsyn,
    message: message::Lifecycle,
) -> Task<Message> {
    match message {
        message::Lifecycle::RetryStartup => {
            if !matches!(app.state.lifecycle, Lifecycle::Failed(_))
                || app.runtime.application.is_some()
            {
                return Task::none();
            }
            let config = match fjarsyn_engine::config::Config::load_or_create() {
                Ok(config) => config,
                Err(error) => {
                    let message = format!("Failed to reload configuration: {error}");
                    app.state.lifecycle = Lifecycle::Failed(message.clone());
                    app.state.notify_error(message);
                    return Task::none();
                }
            };
            app.state.config = config.clone();
            app.state.lifecycle = Lifecycle::Starting;
            Fjarsyn::start_runtime_task(config, app.runtime.event_tx.clone())
        }
        message::Lifecycle::RestartRequested => restart(app),
        message::Lifecycle::ExitRequested => {
            if matches!(app.state.lifecycle, Lifecycle::RestartFailed(_)) {
                iced::exit()
            } else {
                Task::none()
            }
        }
    }
}

fn restart(app: &mut Fjarsyn) -> Task<Message> {
    let codec_restart_required = app.state.screen_share.codec_restart_required();
    let Some(runtime) = relaunch::begin(
        &mut app.state.lifecycle,
        codec_restart_required,
        &mut app.runtime.application,
    ) else {
        return Task::none();
    };

    // Take the sole runtime owner before scheduling any asynchronous work.
    // This makes duplicate restart requests harmless and ensures a replacement
    // cannot launch until every application service has finished shutting down.
    Task::future(async move {
        let outcome = relaunch::shutdown_then_launch(
            async move {
                match runtime {
                    Some(runtime) => runtime.shutdown().await,
                    None => Ok(()),
                }
            },
            relaunch::relaunch_current_executable,
        )
        .await;
        Message::Runtime(message::Runtime::RestartFinished {
            shutdown_warning: outcome.shutdown_warning,
            launch_result: outcome.launch_result,
        })
    })
}

pub(in crate::ui::shell) fn shutdown(app: &mut Fjarsyn) -> Task<Message> {
    if matches!(app.state.lifecycle, Lifecycle::ShuttingDown | Lifecycle::Restarting) {
        return Task::none();
    }
    app.state.lifecycle = Lifecycle::ShuttingDown;
    let Some(runtime) = app.runtime.application.take() else {
        return iced::exit();
    };
    Task::future(async move {
        Message::Runtime(message::Runtime::ShutdownFinished(
            runtime.shutdown().await.map_err(std::sync::Arc::new),
        ))
    })
}
