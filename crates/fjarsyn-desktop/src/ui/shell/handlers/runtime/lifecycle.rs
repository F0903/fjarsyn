use std::sync::Arc;

use iced::Task;

use super::event;
use crate::ui::{
    message::{self, Message},
    runtime,
    screens::Active,
    shell::{Fjarsyn, Lifecycle, Runtime, relaunch},
};

pub(in crate::ui::shell) fn handle_runtime_msg(
    app: &mut Fjarsyn,
    message: message::Runtime,
) -> Task<Message> {
    match message {
        message::Runtime::Initialized { runtime_id, result } => initialize(app, runtime_id, result),
        message::Runtime::EngineStateChanged { runtime_id } => {
            if accepts_engine_output(&app.state.lifecycle, &app.runtime, runtime_id)
                && let Some(state) =
                    app.runtime.engine.as_mut().map(runtime::EngineRuntime::latest_state)
            {
                event::apply_engine_state(app, state);
            }
            Task::none()
        }
        message::Runtime::EngineNotice { runtime_id, notice } => {
            if accepts_engine_output(&app.state.lifecycle, &app.runtime, runtime_id) {
                event::apply_notice(app, notice);
            }
            Task::none()
        }
        message::Runtime::EngineAdapterFailed { runtime_id, failure } => {
            if accepts_engine_output(&app.state.lifecycle, &app.runtime, runtime_id) {
                event::apply_failure(app, failure);
            }
            Task::none()
        }
        message::Runtime::ShutdownFinished(result) => finish_shutdown(result),
        message::Runtime::RestartShutdownFinished(result) => finish_restart_shutdown(app, result),
    }
}

fn initialize(
    app: &mut Fjarsyn,
    runtime_id: runtime::RuntimeId,
    result: Result<runtime::RuntimeSlot, Arc<String>>,
) -> Task<Message> {
    match result {
        Ok(slot) => {
            let Some(mut engine_runtime) = slot.take() else {
                return Task::none();
            };
            if awaits_startup_for_shutdown(&app.state.lifecycle, &app.runtime, runtime_id) {
                app.runtime.reject_startup(runtime_id);
                return shutdown_runtime_then_exit(engine_runtime);
            }
            if !accepts_startup(&app.state.lifecycle, &app.runtime, runtime_id) {
                return shutdown_stale_runtime(engine_runtime);
            }
            if engine_runtime.runtime_id() != runtime_id {
                app.runtime.reject_startup(runtime_id);
                app.state.lifecycle = Lifecycle::StartupFailed(
                    "runtime startup returned an inconsistent runtime ID".into(),
                );
                return shutdown_stale_runtime(engine_runtime);
            }
            let engine_state = engine_runtime.latest_state();
            app.state.settings.engine = engine_runtime.active_settings().clone();
            app.state.local_peer_id = Some(engine_runtime.local_peer_id().clone());
            app.state.local_public_key = Some(engine_runtime.local_public_key().to_owned());
            app.state.contact_projection = Some(engine_runtime.contacts().projection());
            app.state.presence = engine_state.presence;
            app.state.sessions = engine_state.sessions;
            app.state.messaging = engine_state.messaging;
            app.state.screen_share = engine_state.screen_share;
            app.runtime.activate(runtime_id);
            app.runtime.engine = Some(engine_runtime);
            app.state.lifecycle = Lifecycle::Ready;
            finish_startup_route(app);
        }
        Err(error) if accepts_startup(&app.state.lifecycle, &app.runtime, runtime_id) => {
            app.runtime.reject_startup(runtime_id);
            app.state.lifecycle = Lifecycle::StartupFailed(error.to_string());
        }
        Err(_) if awaits_startup_for_shutdown(&app.state.lifecycle, &app.runtime, runtime_id) => {
            app.runtime.reject_startup(runtime_id);
            return iced::exit();
        }
        Err(_) => {}
    }
    Task::none()
}

fn finish_startup_route(app: &mut Fjarsyn) {
    if app.active_screen.is_settings() {
        app.active_screen = Active::from_route(message::Route::Home, app.state.presentation());
    }
}

fn shutdown_stale_runtime(engine_runtime: runtime::EngineRuntime) -> Task<Message> {
    Task::future(async move {
        if let Err(error) = engine_runtime.shutdown().await {
            tracing::warn!(%error, "stale desktop runtime shut down with errors");
        }
        Message::NoOp
    })
}

fn shutdown_runtime_then_exit(engine_runtime: runtime::EngineRuntime) -> Task<Message> {
    Task::future(async move {
        let result = engine_runtime.shutdown().await.map_err(Arc::new);
        Message::Runtime(message::Runtime::ShutdownFinished(result))
    })
}

fn accepts_startup(
    lifecycle: &Lifecycle,
    runtime: &Runtime,
    runtime_id: runtime::RuntimeId,
) -> bool {
    matches!(lifecycle, Lifecycle::Starting) && runtime.expects_startup(runtime_id)
}

fn accepts_engine_output(
    lifecycle: &Lifecycle,
    runtime: &Runtime,
    runtime_id: runtime::RuntimeId,
) -> bool {
    lifecycle.accepts_engine_actions() && runtime.is_active(runtime_id)
}

fn awaits_startup_for_shutdown(
    lifecycle: &Lifecycle,
    runtime: &Runtime,
    runtime_id: runtime::RuntimeId,
) -> bool {
    matches!(lifecycle, Lifecycle::ShuttingDown) && runtime.expects_startup(runtime_id)
}

fn finish_shutdown(result: Result<(), Arc<String>>) -> Task<Message> {
    if let Err(error) = result {
        tracing::warn!("engine runtime shutdown completed with errors: {error}");
    }
    iced::exit()
}

fn finish_restart_shutdown(
    app: &mut Fjarsyn,
    shutdown_result: Result<(), Arc<String>>,
) -> Task<Message> {
    if let Err(error) = shutdown_result {
        tracing::warn!("engine runtime shutdown completed with errors before restart: {error}");
    }
    let Some(after_shutdown) = relaunch::shutdown_finished(&app.state.lifecycle) else {
        return Task::none();
    };
    if matches!(after_shutdown, relaunch::AfterShutdown::Exit) {
        return iced::exit();
    }

    let launch_result = relaunch::relaunch_current_executable().map_err(Arc::new);
    let should_exit = relaunch::finish(&mut app.state.lifecycle, &launch_result)
        .expect("restart lifecycle remains active until replacement launch");
    if should_exit { iced::exit() } else { Task::none() }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        accepts_engine_output, accepts_startup, awaits_startup_for_shutdown, finish_startup_route,
        initialize,
    };
    use crate::{
        settings::{Settings, Store},
        ui::{
            message::Route,
            runtime::RuntimeId,
            screens::Active,
            shell::{Fjarsyn, Lifecycle, Runtime},
        },
    };

    fn starting_settings_app(runtime_id: RuntimeId) -> Fjarsyn {
        let path = std::env::temp_dir()
            .join(format!("fjarsyn-startup-route-{}.json", uuid::Uuid::new_v4()));
        let mut app = Fjarsyn::new(Settings::default(), Store::at(path), runtime_id);
        app.active_screen = Active::from_route(Route::Settings, app.state.presentation());
        app
    }

    #[test]
    fn startup_completion_requires_the_expected_runtime_id() {
        let expected = RuntimeId::next();
        let stale = RuntimeId::next();
        let runtime = Runtime::awaiting(expected);

        assert!(accepts_startup(&Lifecycle::Starting, &runtime, expected));
        assert!(!accepts_startup(&Lifecycle::Starting, &runtime, stale));
        assert!(!accepts_startup(&Lifecycle::StartupFailed("failed".into()), &runtime, expected,));
        assert!(!accepts_startup(&Lifecycle::Ready, &runtime, expected));
    }

    #[test]
    fn a_new_retry_runtime_id_supersedes_the_failed_attempt() {
        let old = RuntimeId::next();
        let mut runtime = Runtime::awaiting(old);
        runtime.reject_startup(old);
        let new = runtime.expect_new_startup();

        assert!(!accepts_startup(&Lifecycle::Starting, &runtime, old));
        assert!(accepts_startup(&Lifecycle::Starting, &runtime, new));
    }

    #[test]
    fn repeated_startup_failure_preserves_the_recovery_editor() {
        let runtime_id = RuntimeId::next();
        let mut app = starting_settings_app(runtime_id);

        drop(initialize(&mut app, runtime_id, Err(Arc::new("retry failed".into()))));

        assert_eq!(app.state.lifecycle, Lifecycle::StartupFailed("retry failed".into()));
        assert_eq!(app.active_screen.route(), Route::Settings);
    }

    #[test]
    fn successful_recovery_selects_home() {
        let runtime_id = RuntimeId::next();
        let mut app = starting_settings_app(runtime_id);

        finish_startup_route(&mut app);

        assert_eq!(app.active_screen.route(), Route::Home);
    }

    #[test]
    fn engine_output_requires_the_active_runtime_id() {
        let active = RuntimeId::next();
        let stale = RuntimeId::next();
        let mut runtime = Runtime::awaiting(active);
        runtime.activate(active);

        assert!(accepts_engine_output(&Lifecycle::Ready, &runtime, active));
        assert!(!accepts_engine_output(&Lifecycle::Ready, &runtime, stale));
        assert!(!accepts_engine_output(
            &Lifecycle::Degraded("engine adapter failed".into()),
            &runtime,
            active,
        ));
        assert!(!accepts_engine_output(&Lifecycle::ShuttingDown, &runtime, active));
    }

    #[test]
    fn shutdown_waits_only_for_the_expected_runtime_id() {
        let expected = RuntimeId::next();
        let stale = RuntimeId::next();
        let runtime = Runtime::awaiting(expected);

        assert!(awaits_startup_for_shutdown(&Lifecycle::ShuttingDown, &runtime, expected,));
        assert!(!awaits_startup_for_shutdown(&Lifecycle::ShuttingDown, &runtime, stale,));
        assert!(!awaits_startup_for_shutdown(&Lifecycle::Starting, &runtime, expected));
    }
}
