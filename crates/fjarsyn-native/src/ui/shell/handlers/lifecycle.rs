use iced::Task;

use crate::ui::{
    message::{LifecycleMessage, Message, RuntimeMessage},
    shell::{AppLifecycle, Fjarsyn, relaunch},
};

pub fn handle_lifecycle_msg(app: &mut Fjarsyn, message: LifecycleMessage) -> Task<Message> {
    match message {
        LifecycleMessage::RetryStartup => {
            if !matches!(app.ctx.lifecycle, AppLifecycle::Failed(_))
                || app.runtime.application.is_some()
            {
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
        LifecycleMessage::RestartRequested => restart(app),
        LifecycleMessage::ExitRequested => {
            if matches!(app.ctx.lifecycle, AppLifecycle::RestartFailed(_)) {
                iced::exit()
            } else {
                Task::none()
            }
        }
    }
}

fn restart(app: &mut Fjarsyn) -> Task<Message> {
    let codec_restart_required = app.ctx.media.codec_restart_required();
    let Some(attempt) =
        begin_restart(&mut app.ctx.lifecycle, codec_restart_required, &mut app.runtime.application)
    else {
        return Task::none();
    };

    // Take the sole runtime owner before scheduling any asynchronous work.
    // This makes duplicate restart requests harmless and ensures a replacement
    // cannot launch until every application service has finished shutting down.
    let runtime = attempt.runtime;
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
        Message::Runtime(RuntimeMessage::RestartFinished {
            shutdown_warning: outcome.shutdown_warning,
            launch_result: outcome.launch_result,
        })
    })
}

struct RestartAttempt<Runtime> {
    runtime: Option<Runtime>,
}

fn begin_restart<Runtime>(
    lifecycle: &mut AppLifecycle,
    codec_restart_required: bool,
    runtime: &mut Option<Runtime>,
) -> Option<RestartAttempt<Runtime>> {
    if !can_begin_restart(lifecycle, codec_restart_required) {
        return None;
    }
    let runtime = runtime.take();
    *lifecycle = AppLifecycle::Restarting;
    Some(RestartAttempt { runtime })
}

fn can_begin_restart(lifecycle: &AppLifecycle, codec_restart_required: bool) -> bool {
    matches!(lifecycle, AppLifecycle::RestartFailed(_))
        || codec_restart_required && matches!(lifecycle, AppLifecycle::Ready)
}

pub fn shutdown(app: &mut Fjarsyn) -> Task<Message> {
    if matches!(app.ctx.lifecycle, AppLifecycle::ShuttingDown | AppLifecycle::Restarting) {
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

#[cfg(test)]
mod tests {
    use super::{AppLifecycle, begin_restart, can_begin_restart};

    #[test]
    fn restart_can_only_begin_from_a_running_app_or_a_failed_relaunch() {
        assert!(can_begin_restart(&AppLifecycle::Ready, true));
        assert!(!can_begin_restart(&AppLifecycle::Ready, false));
        assert!(can_begin_restart(&AppLifecycle::RestartFailed("launch failed".into()), false,));

        for lifecycle in [
            AppLifecycle::Starting,
            AppLifecycle::Failed("startup failed".into()),
            AppLifecycle::ShuttingDown,
            AppLifecycle::Restarting,
        ] {
            assert!(!can_begin_restart(&lifecycle, true));
        }
    }

    #[test]
    fn a_restart_attempt_takes_the_runtime_exactly_once() {
        let mut lifecycle = AppLifecycle::Ready;
        let mut runtime = Some("runtime owner");

        let first = begin_restart(&mut lifecycle, true, &mut runtime).unwrap();

        assert_eq!(first.runtime, Some("runtime owner"));
        assert!(runtime.is_none());
        assert_eq!(lifecycle, AppLifecycle::Restarting);
        assert!(begin_restart(&mut lifecycle, true, &mut runtime).is_none());
    }

    #[test]
    fn a_failed_relaunch_retries_without_recreating_application_services() {
        let mut lifecycle = AppLifecycle::RestartFailed("launch failed".into());
        let mut runtime: Option<&str> = None;

        let retry = begin_restart(&mut lifecycle, false, &mut runtime).unwrap();

        assert!(retry.runtime.is_none());
        assert!(runtime.is_none());
        assert_eq!(lifecycle, AppLifecycle::Restarting);
    }
}
