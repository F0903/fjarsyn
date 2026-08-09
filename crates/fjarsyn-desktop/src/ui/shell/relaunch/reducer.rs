use std::sync::Arc;

use crate::ui::shell::Lifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::shell) enum AfterShutdown {
    LaunchReplacement,
    Exit,
}

/// Admits one restart transition and takes the current runtime owner.
///
/// The outer option reports admission. The inner option is absent when a
/// failed replacement launch is retried after the old runtime already shut
/// down.
pub(in crate::ui::shell) fn begin<Owner>(
    lifecycle: &mut Lifecycle,
    restart_required: bool,
    runtime: &mut Option<Owner>,
) -> Option<Option<Owner>> {
    let admitted = matches!(lifecycle, Lifecycle::RestartFailed(_) | Lifecycle::Degraded(_))
        || restart_required && matches!(lifecycle, Lifecycle::Ready);
    if !admitted {
        return None;
    }

    *lifecycle = Lifecycle::Restarting;
    Some(runtime.take())
}

/// Resolves the race between restart shutdown and an intervening close.
pub(in crate::ui::shell) fn shutdown_finished(lifecycle: &Lifecycle) -> Option<AfterShutdown> {
    match lifecycle {
        Lifecycle::Restarting => Some(AfterShutdown::LaunchReplacement),
        Lifecycle::ShuttingDown => Some(AfterShutdown::Exit),
        _ => None,
    }
}

/// Applies a replacement-launch completion.
///
/// `None` rejects a stale completion. An admitted completion returns whether
/// the current process should exit after a successful replacement launch.
pub(in crate::ui::shell) fn finish(
    lifecycle: &mut Lifecycle,
    launch_result: &Result<(), Arc<String>>,
) -> Option<bool> {
    if !matches!(lifecycle, Lifecycle::Restarting) {
        return None;
    }
    match launch_result {
        Ok(()) => {
            *lifecycle = Lifecycle::ShuttingDown;
            Some(true)
        }
        Err(error) => {
            *lifecycle = Lifecycle::RestartFailed(error.to_string());
            Some(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AfterShutdown, Lifecycle, begin, finish, shutdown_finished};

    #[test]
    fn restart_can_only_begin_from_a_running_degraded_or_failed_relaunch_state() {
        let mut ready = Lifecycle::Ready;
        let mut runtime = Some("runtime");
        assert!(begin(&mut ready, true, &mut runtime).is_some());

        let mut ready_without_restart = Lifecycle::Ready;
        let mut runtime = Some("runtime");
        assert!(begin(&mut ready_without_restart, false, &mut runtime).is_none());

        let mut degraded = Lifecycle::Degraded("engine adapter stopped".into());
        let mut runtime = Some("runtime");
        assert_eq!(begin(&mut degraded, false, &mut runtime), Some(Some("runtime")));
        assert_eq!(degraded, Lifecycle::Restarting);

        let mut retry = Lifecycle::RestartFailed("launch failed".into());
        let mut no_runtime: Option<&str> = None;
        assert_eq!(begin(&mut retry, false, &mut no_runtime), Some(None));
        assert!(no_runtime.is_none());
        assert_eq!(retry, Lifecycle::Restarting);

        for mut lifecycle in [
            Lifecycle::Starting,
            Lifecycle::StartupFailed("startup failed".into()),
            Lifecycle::ShuttingDown,
            Lifecycle::Restarting,
        ] {
            let mut runtime = Some("runtime");
            assert!(begin(&mut lifecycle, true, &mut runtime).is_none());
        }
    }

    #[test]
    fn a_restart_transition_takes_the_runtime_exactly_once() {
        let mut lifecycle = Lifecycle::Ready;
        let mut runtime = Some("runtime owner");

        let first = begin(&mut lifecycle, true, &mut runtime).unwrap();

        assert_eq!(first, Some("runtime owner"));
        assert!(runtime.is_none());
        assert_eq!(lifecycle, Lifecycle::Restarting);
        assert!(begin(&mut lifecycle, true, &mut runtime).is_none());
    }

    #[test]
    fn stale_restart_completion_is_ignored() {
        let mut lifecycle = Lifecycle::Ready;

        let should_exit = finish(&mut lifecycle, &Err(Arc::new("stale failure".into())));

        assert_eq!(should_exit, None);
        assert_eq!(lifecycle, Lifecycle::Ready);
    }

    #[test]
    fn successful_replacement_requests_exit_only_after_launch_completion() {
        let mut lifecycle = Lifecycle::Restarting;

        let should_exit = finish(&mut lifecycle, &Ok(()));

        assert_eq!(should_exit, Some(true));
        assert_eq!(lifecycle, Lifecycle::ShuttingDown);
        assert_eq!(shutdown_finished(&lifecycle), Some(AfterShutdown::Exit));
    }

    #[test]
    fn close_while_restart_shutdown_is_running_suppresses_replacement_launch() {
        assert_eq!(
            shutdown_finished(&Lifecycle::Restarting),
            Some(AfterShutdown::LaunchReplacement)
        );
        assert_eq!(shutdown_finished(&Lifecycle::ShuttingDown), Some(AfterShutdown::Exit));
        assert_eq!(shutdown_finished(&Lifecycle::Ready), None);
    }

    #[test]
    fn launch_failure_enters_an_inert_retryable_state() {
        let mut lifecycle = Lifecycle::Restarting;

        let should_exit = finish(&mut lifecycle, &Err(Arc::new("launch failed".into())));

        assert_eq!(should_exit, Some(false));
        assert_eq!(lifecycle, Lifecycle::RestartFailed("launch failed".into()));
    }
}
