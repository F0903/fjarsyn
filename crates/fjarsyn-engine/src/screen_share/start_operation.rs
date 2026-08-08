use std::{
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
};

use futures::FutureExt;
use tokio::{sync::oneshot, task::JoinHandle};

use super::{Config, SelectionKey, local};
use crate::{
    media::capture::PlatformItem,
    peer_session::{self, LocalShareState, ShareId},
};

pub(super) struct StartOutcome {
    pub(super) local: local::Controller,
    pub(super) result: Result<(), String>,
}

/// Service-owned supervision for one local start transaction.
pub(super) struct StartOperation {
    selection: SelectionKey,
    response: Option<oneshot::Sender<Result<(), String>>>,
    task: Option<JoinHandle<StartOutcome>>,
    cancel_on_drop: bool,
    failure: Arc<Mutex<Option<String>>>,
}

impl StartOperation {
    pub(super) fn spawn(
        local: local::Controller,
        selection: SelectionKey,
        item: PlatformItem,
        sessions: peer_session::ServiceHandle,
        config: Config,
        deadline: tokio::time::Instant,
        response: oneshot::Sender<Result<(), String>>,
    ) -> Self {
        let failure = Arc::new(Mutex::new(None));
        let task_selection = selection.clone();
        let task_failure = failure.clone();
        let task = tokio::spawn(run_start(
            local,
            task_selection,
            item,
            sessions,
            config,
            deadline,
            task_failure,
        ));
        Self {
            selection,
            response: Some(response),
            task: Some(task),
            cancel_on_drop: true,
            failure,
        }
    }

    pub(super) fn selection(&self) -> &SelectionKey {
        &self.selection
    }

    pub(super) fn cancel(&self) {
        self.selection.cancel();
    }

    pub(super) fn fail(&self, reason: String) {
        self.failure.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_or_insert(reason);
        self.selection.cancel();
    }

    pub(super) fn take_failure(&self) -> Option<String> {
        self.failure.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }

    pub(super) async fn wait(&mut self) -> Result<StartOutcome, tokio::task::JoinError> {
        self.task.as_mut().expect("active start operation owns its task").await
    }

    pub(super) fn respond(&mut self, result: Result<(), String>) -> bool {
        if let Some(response) = self.response.take() {
            response.send(result).is_ok()
        } else {
            false
        }
    }

    pub(super) fn finish(&mut self) {
        self.task.take();
        self.cancel_on_drop = false;
    }
}

impl Drop for StartOperation {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.selection.cancel();
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_start(
    mut local: local::Controller,
    selection: SelectionKey,
    item: PlatformItem,
    sessions: peer_session::ServiceHandle,
    config: Config,
    deadline: tokio::time::Instant,
    failure: Arc<Mutex<Option<String>>>,
) -> StartOutcome {
    let mut committed_share = None;
    let execution = AssertUnwindSafe(async {
        ensure_operation_current(&selection, deadline)?;
        let share_id = resolve_started_share(&sessions, &selection).await?;
        committed_share = Some(share_id);
        ensure_operation_current(&selection, deadline)?;
        let sink = tokio::select! {
            biased;
            _ = selection.cancelled() => return Err("screen-share start was cancelled".into()),
            result = sessions.encoded_video_sink(selection.session_id(), share_id) => {
                result.map_err(|error| error.to_string())?
            }
        };
        local.start(&selection, item, sink, config).await?;
        ensure_operation_current(&selection, deadline)?;
        Ok(())
    })
    .catch_unwind()
    .await;
    let mut result: Result<(), String> =
        execution.unwrap_or_else(|_| Err("screen-share start operation panicked".into()));
    let requested_failure = failure.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(reason) = &requested_failure {
        result = Err(reason.clone());
    }

    if let Err(reason) = &result {
        let cancelled = selection.is_cancelled();
        let report_failure = requested_failure.is_some() || !cancelled;
        if report_failure {
            let _ = local.fail_selection(&selection, reason.clone()).await;
        }
        let binding = local.abort_start(&selection).await;
        if report_failure && binding.is_some() {
            local.report_failure(selection.session_id(), reason.clone()).await;
        }
        let share_id = binding
            .map(|binding| binding.share_id())
            .or(committed_share)
            .or_else(|| active_local_share(&sessions.snapshot(), selection.session_id()));
        if let Some(share_id) = share_id {
            stop_share(&sessions, selection.session_id(), share_id).await;
        }
    }

    StartOutcome { local, result }
}

async fn resolve_started_share(
    sessions: &peer_session::ServiceHandle,
    selection: &SelectionKey,
) -> Result<ShareId, String> {
    if selection.is_cancelled() {
        return Err("screen-share start was cancelled".into());
    }
    let session_id = selection.session_id();
    let mut snapshots = sessions.subscribe();
    // Once the peer-session service accepts this mutation, its future must be
    // drained to a definitive result. Cancellation is observed immediately
    // afterward so a returned ShareId can be rolled back exactly.
    let start = sessions.start_screen_share(session_id).await;
    match start {
        Ok(share_id) => Ok(share_id),
        Err(
            error @ (peer_session::Error::OutcomeUnknown | peer_session::Error::ResponseDropped),
        ) => {
            if let Some(share_id) = active_local_share(&snapshots.borrow(), session_id) {
                return Ok(share_id);
            }
            let observed = async {
                loop {
                    snapshots.changed().await.ok()?;
                    if let Some(share_id) = active_local_share(&snapshots.borrow(), session_id) {
                        return Some(share_id);
                    }
                    snapshots.borrow().session(session_id)?;
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), observed)
                .await
                .ok()
                .flatten()
                .ok_or_else(|| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn ensure_operation_current(
    selection: &SelectionKey,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    if selection.is_cancelled() {
        Err("screen-share start was cancelled".into())
    } else if tokio::time::Instant::now() >= deadline {
        Err("screen-share startup did not complete in time".into())
    } else {
        Ok(())
    }
}

fn active_local_share(
    snapshot: &peer_session::Snapshot,
    session_id: peer_session::SessionId,
) -> Option<ShareId> {
    snapshot.session(session_id).and_then(|session| match session.local_share {
        LocalShareState::Active { share_id, .. } => Some(share_id),
        LocalShareState::Inactive => None,
    })
}

pub(super) async fn stop_share(
    sessions: &peer_session::ServiceHandle,
    session_id: peer_session::SessionId,
    share_id: ShareId,
) {
    if let Err(error) = sessions.stop_screen_share(session_id, share_id).await
        && !matches!(
            error,
            peer_session::Error::SessionNotFound(_) | peer_session::Error::ShareMismatch(_)
        )
    {
        tracing::debug!(
            %session_id,
            %share_id,
            %error,
            "screen-share stop remains pending reconciliation"
        );
    }
}
