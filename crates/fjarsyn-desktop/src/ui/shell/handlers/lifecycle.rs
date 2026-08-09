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
            if !accepts_plain_startup_retry(
                &app.state.lifecycle,
                app.active_screen.is_settings(),
                app.runtime.engine.is_some(),
            ) {
                Task::none()
            } else {
                begin_startup_retry(app)
            }
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

pub(super) fn begin_startup_retry(app: &mut Fjarsyn) -> Task<Message> {
    if !can_begin_startup_retry(&app.state.lifecycle, app.runtime.engine.is_some()) {
        return Task::none();
    }
    let settings = app.state.settings.engine.clone();
    let runtime_id = app.runtime.expect_new_startup();
    app.state.lifecycle = Lifecycle::Starting;
    Fjarsyn::start_runtime_task(runtime_id, settings)
}

fn can_begin_startup_retry(lifecycle: &Lifecycle, has_runtime: bool) -> bool {
    matches!(lifecycle, Lifecycle::StartupFailed(_)) && !has_runtime
}

fn accepts_plain_startup_retry(
    lifecycle: &Lifecycle,
    startup_settings_active: bool,
    has_runtime: bool,
) -> bool {
    !startup_settings_active && can_begin_startup_retry(lifecycle, has_runtime)
}

fn restart(app: &mut Fjarsyn) -> Task<Message> {
    let restart_required = app.state.screen_share.codec_restart_required();
    let Some(runtime) =
        relaunch::begin(&mut app.state.lifecycle, restart_required, &mut app.runtime.engine)
    else {
        return Task::none();
    };
    app.runtime.clear_ids();

    // Take the sole runtime owner before scheduling any asynchronous work.
    // This makes duplicate restart requests harmless and ensures a replacement
    // cannot launch until every application service has finished shutting down.
    Task::future(async move {
        let result = match runtime {
            Some(runtime) => runtime.shutdown().await,
            None => Ok(()),
        }
        .map_err(std::sync::Arc::new);
        Message::Runtime(message::Runtime::RestartShutdownFinished(result))
    })
}

pub(in crate::ui::shell) fn shutdown(app: &mut Fjarsyn) -> Task<Message> {
    if matches!(app.state.lifecycle, Lifecycle::ShuttingDown) {
        return Task::none();
    }
    if matches!(app.state.lifecycle, Lifecycle::Restarting) {
        // The restart task already owns and is shutting down the runtime. The
        // completion handler observes this transition and exits without
        // launching a replacement process.
        app.state.lifecycle = Lifecycle::ShuttingDown;
        return Task::none();
    }
    let startup_is_pending =
        matches!(app.state.lifecycle, Lifecycle::Starting) && app.runtime.engine.is_none();
    app.state.lifecycle = Lifecycle::ShuttingDown;
    if startup_is_pending {
        // The startup task may already own a completed engine runtime. Keep its
        // expected runtime ID alive so its completion can be consumed and the
        // owner can be shut down before exiting.
        return Task::none();
    }
    app.runtime.clear_ids();
    let Some(runtime) = app.runtime.engine.take() else {
        return iced::exit();
    };
    Task::future(async move {
        let result = runtime.shutdown().await.map_err(std::sync::Arc::new);
        Message::Runtime(message::Runtime::ShutdownFinished(result))
    })
}

#[cfg(test)]
mod tests {
    use super::{Lifecycle, accepts_plain_startup_retry, can_begin_startup_retry};

    #[test]
    fn plain_retry_belongs_only_to_the_startup_failure_overview() {
        let failed = Lifecycle::StartupFailed("failed".into());

        assert!(accepts_plain_startup_retry(&failed, false, false));
        assert!(!accepts_plain_startup_retry(&failed, true, false));
        assert!(!accepts_plain_startup_retry(&failed, false, true));
        assert!(!accepts_plain_startup_retry(&Lifecycle::Starting, false, false));
    }

    #[test]
    fn persisted_recovery_settings_can_begin_a_retry_from_the_settings_route() {
        assert!(can_begin_startup_retry(&Lifecycle::StartupFailed("failed".into()), false,));
        assert!(!can_begin_startup_retry(&Lifecycle::Ready, false));
    }
}
