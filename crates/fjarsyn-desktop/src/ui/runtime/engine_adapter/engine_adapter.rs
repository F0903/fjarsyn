use std::{any::Any, future::Future, panic::AssertUnwindSafe};

use fjarsyn_engine::{messaging, peer_session, presence, screen_share};
use futures::FutureExt as _;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

use super::{
    coordinator::{Coordinator, Exit},
    engine_state::EngineState,
    failure::{Failure, Source},
    notice::Notice,
};
use crate::ui::{
    runtime::{Retained, RuntimeId},
    subscription::Receiver,
};

const NOTICE_CAPACITY: usize = 64;

/// Per-runtime receivers installed only after the initial engine state is applied.
#[derive(Clone)]
pub(in crate::ui) struct Receivers {
    pub(in crate::ui) runtime_id: RuntimeId,
    pub(in crate::ui) state: Retained<EngineState>,
    pub(in crate::ui) notices: Receiver<Notice>,
    pub(in crate::ui) failures: Receiver<Failure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::runtime) enum Shutdown {
    Clean,
    Failed(Failure),
    TimedOut,
}

/// Owns the engine-output coordinator and its cooperative shutdown signal.
pub(in crate::ui::runtime) struct EngineAdapter {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), Failure>>>,
}

impl EngineAdapter {
    pub(in crate::ui::runtime) fn start(
        runtime_id: RuntimeId,
        presence: &presence::ServiceHandle,
        sessions: &peer_session::ServiceHandle,
        messaging: &messaging::ServiceHandle,
        screen_share: &screen_share::ServiceHandle,
    ) -> (Self, Receivers) {
        let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CAPACITY);
        let (failure_tx, failure_rx) = mpsc::channel(1);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (coordinator, state_rx) =
            Coordinator::prepare(presence, sessions, messaging, screen_share, notice_tx);
        let task = spawn_supervised(coordinator.run(shutdown_rx), failure_tx);
        let receivers = Receivers {
            runtime_id,
            state: Retained::new(state_rx),
            notices: Receiver::new(notice_rx),
            failures: Receiver::new(failure_rx),
        };

        (Self { shutdown: Some(shutdown_tx), task: Some(task) }, receivers)
    }

    pub(in crate::ui::runtime) async fn shutdown_until(&mut self, deadline: Instant) -> Shutdown {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let Some(mut task) = self.task.take() else {
            return Shutdown::Clean;
        };
        match tokio::time::timeout_at(deadline, &mut task).await {
            Ok(Ok(Ok(()))) => Shutdown::Clean,
            Ok(Ok(Err(failure))) => Shutdown::Failed(failure),
            Ok(Err(error)) if error.is_panic() => {
                Shutdown::Failed(Failure::panicked(Source::Adapter, error.to_string()))
            }
            Ok(Err(_)) => Shutdown::Failed(Failure::unexpected_exit(Source::Adapter)),
            Err(_) => {
                task.abort();
                Shutdown::TimedOut
            }
        }
    }

    pub(in crate::ui::runtime) fn abort(&mut self) {
        self.shutdown.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Drop for EngineAdapter {
    fn drop(&mut self) {
        self.abort();
    }
}

fn spawn_supervised<F>(run: F, failure_tx: mpsc::Sender<Failure>) -> JoinHandle<Result<(), Failure>>
where
    F: Future<Output = Result<Exit, Failure>> + Send + 'static,
{
    tokio::spawn(async move {
        let outcome = AssertUnwindSafe(run).catch_unwind().await;
        let result = match outcome {
            Ok(Ok(Exit::Shutdown)) => Ok(()),
            #[cfg(test)]
            Ok(Ok(Exit::Unexpected)) => Err(Failure::unexpected_exit(Source::Adapter)),
            Ok(Err(failure)) => Err(failure),
            Err(payload) => {
                Err(Failure::panicked(Source::Adapter, panic_message(payload.as_ref())))
            }
        };
        if let Err(failure) = &result {
            let _ = failure_tx.send(failure.clone()).await;
        }
        result
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".into())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn source_failure_is_published_immediately() {
        let (failure_tx, mut failure_rx) = mpsc::channel(1);
        let expected = Failure::source_closed(Source::PresenceState);
        let task = spawn_supervised(async move { Err(expected.clone()) }, failure_tx);

        let reported =
            tokio::time::timeout(Duration::from_secs(1), failure_rx.recv()).await.unwrap().unwrap();

        assert_eq!(reported.source(), Source::PresenceState);
        assert_eq!(task.await.unwrap().unwrap_err(), reported);
    }

    #[tokio::test]
    async fn panic_and_unexpected_exit_are_typed_failures() {
        let (panic_tx, mut panic_rx) = mpsc::channel(1);
        let panic_task = spawn_supervised(
            async {
                panic!("engine adapter panic");
                #[allow(unreachable_code)]
                Ok(Exit::Shutdown)
            },
            panic_tx,
        );
        let panic_failure = panic_rx.recv().await.unwrap();
        assert_eq!(panic_failure.source(), Source::Adapter);
        assert!(panic_task.await.unwrap().is_err());

        let (exit_tx, mut exit_rx) = mpsc::channel(1);
        let exit_task = spawn_supervised(async { Ok(Exit::Unexpected) }, exit_tx);
        let exit_failure = exit_rx.recv().await.unwrap();
        assert_eq!(exit_failure.source(), Source::Adapter);
        assert!(exit_task.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn intentional_shutdown_exit_emits_no_failure() {
        let (failure_tx, mut failure_rx) = mpsc::channel(1);
        let task = spawn_supervised(async { Ok(Exit::Shutdown) }, failure_tx);

        assert!(task.await.unwrap().is_ok());
        assert!(failure_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn shutdown_reports_clean_failed_and_timed_out_distinctly() {
        let (clean_tx, clean_rx) = oneshot::channel();
        let clean_task = tokio::spawn(async move {
            let _ = clean_rx.await;
            Ok(())
        });
        let mut clean = EngineAdapter { shutdown: Some(clean_tx), task: Some(clean_task) };
        assert_eq!(
            clean.shutdown_until(Instant::now() + Duration::from_secs(1)).await,
            Shutdown::Clean
        );

        let expected = Failure::source_closed(Source::MessagingState);
        let failed_task = {
            let expected = expected.clone();
            tokio::spawn(async move { Err(expected) })
        };
        let (failed_tx, failed_rx) = oneshot::channel();
        drop(failed_rx);
        let mut failed = EngineAdapter { shutdown: Some(failed_tx), task: Some(failed_task) };
        assert_eq!(
            failed.shutdown_until(Instant::now() + Duration::from_secs(1)).await,
            Shutdown::Failed(expected)
        );

        let (timeout_tx, timeout_rx) = oneshot::channel();
        let timeout_task = tokio::spawn(async move {
            let _shutdown = timeout_rx;
            std::future::pending::<Result<(), Failure>>().await
        });
        let mut timed_out = EngineAdapter { shutdown: Some(timeout_tx), task: Some(timeout_task) };
        assert_eq!(
            timed_out.shutdown_until(Instant::now() + Duration::from_millis(1)).await,
            Shutdown::TimedOut
        );
    }
}
