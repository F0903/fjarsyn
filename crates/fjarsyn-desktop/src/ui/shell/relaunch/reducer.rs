use std::sync::Arc;

use crate::ui::shell::Lifecycle;

/// Admits one restart transition and takes the current runtime owner.
///
/// The outer option reports admission. The inner option is absent when a
/// failed replacement launch is retried after the old runtime already shut
/// down.
pub(in crate::ui::shell) fn begin<Owner>(
    lifecycle: &mut Lifecycle,
    codec_restart_required: bool,
    runtime: &mut Option<Owner>,
) -> Option<Option<Owner>> {
    let admitted = matches!(lifecycle, Lifecycle::RestartFailed(_))
        || codec_restart_required && matches!(lifecycle, Lifecycle::Ready);
    if !admitted {
        return None;
    }

    *lifecycle = Lifecycle::Restarting;
    Some(runtime.take())
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
        Ok(()) => Some(true),
        Err(error) => {
            *lifecycle = Lifecycle::RestartFailed(error.to_string());
            Some(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Lifecycle, begin, finish};

    #[test]
    fn restart_can_only_begin_from_a_running_app_or_a_failed_relaunch() {
        let mut ready = Lifecycle::Ready;
        let mut runtime = Some("runtime");
        assert!(begin(&mut ready, true, &mut runtime).is_some());

        let mut ready_without_restart = Lifecycle::Ready;
        let mut runtime = Some("runtime");
        assert!(begin(&mut ready_without_restart, false, &mut runtime).is_none());

        let mut retry = Lifecycle::RestartFailed("launch failed".into());
        let mut no_runtime: Option<&str> = None;
        assert_eq!(begin(&mut retry, false, &mut no_runtime), Some(None));
        assert!(no_runtime.is_none());
        assert_eq!(retry, Lifecycle::Restarting);

        for mut lifecycle in [
            Lifecycle::Starting,
            Lifecycle::Failed("startup failed".into()),
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
        assert_eq!(lifecycle, Lifecycle::Restarting);
    }

    #[test]
    fn launch_failure_enters_an_inert_retryable_state() {
        let mut lifecycle = Lifecycle::Restarting;

        let should_exit = finish(&mut lifecycle, &Err(Arc::new("launch failed".into())));

        assert_eq!(should_exit, Some(false));
        assert_eq!(lifecycle, Lifecycle::RestartFailed("launch failed".into()));
    }
}
