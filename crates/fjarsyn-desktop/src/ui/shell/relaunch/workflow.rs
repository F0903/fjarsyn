use std::{future::Future, sync::Arc};

pub(in crate::ui::shell) struct Outcome {
    pub(in crate::ui::shell) shutdown_warning: Option<Arc<String>>,
    pub(in crate::ui::shell) launch_result: Result<(), Arc<String>>,
}

pub(in crate::ui::shell) async fn shutdown_then_launch<Shutdown, Launch>(
    shutdown: Shutdown,
    launch: Launch,
) -> Outcome
where
    Shutdown: Future<Output = Result<(), String>>,
    Launch: FnOnce() -> Result<(), String>,
{
    let shutdown_warning = shutdown.await.err().map(Arc::new);
    let launch_result = launch().map_err(Arc::new);
    Outcome { shutdown_warning, launch_result }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::shutdown_then_launch;

    #[tokio::test]
    async fn replacement_launch_waits_for_shutdown_completion() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let shutdown_trace = trace.clone();
        let launch_trace = trace.clone();

        let outcome = shutdown_then_launch(
            async move {
                shutdown_trace.lock().unwrap().push("shutdown started");
                tokio::task::yield_now().await;
                shutdown_trace.lock().unwrap().push("shutdown completed");
                Ok(())
            },
            move || {
                let mut trace = launch_trace.lock().unwrap();
                assert_eq!(&*trace, &["shutdown started", "shutdown completed"]);
                trace.push("replacement launched");
                Ok(())
            },
        )
        .await;

        assert!(outcome.shutdown_warning.is_none());
        assert!(outcome.launch_result.is_ok());
        assert_eq!(
            &*trace.lock().unwrap(),
            &["shutdown started", "shutdown completed", "replacement launched"]
        );
    }

    #[tokio::test]
    async fn shutdown_warning_does_not_skip_the_replacement_attempt() {
        let launched = Arc::new(Mutex::new(false));
        let launch_observer = launched.clone();

        let outcome =
            shutdown_then_launch(async { Err("shutdown warning".to_owned()) }, move || {
                *launch_observer.lock().unwrap() = true;
                Err("launch failed".to_owned())
            })
            .await;

        assert_eq!(
            outcome.shutdown_warning.as_deref().map(String::as_str),
            Some("shutdown warning")
        );
        assert_eq!(outcome.launch_result.unwrap_err().as_str(), "launch failed");
        assert!(*launched.lock().unwrap());
    }
}
