//! Service-owned Tokio tasks supervising native codec worker threads.

use std::{collections::HashMap, future::Future};

use tokio::task::{Id, JoinSet};

use crate::media::codec::{Direction, registry::WorkerId};

pub(in crate::media::codec) struct SupervisorTasks {
    tasks: JoinSet<()>,
    workers: HashMap<Id, (WorkerId, Direction)>,
}

impl SupervisorTasks {
    pub(in crate::media::codec) fn new() -> Self {
        Self { tasks: JoinSet::new(), workers: HashMap::new() }
    }

    pub(in crate::media::codec) fn spawn<Task>(
        &mut self,
        worker_id: WorkerId,
        direction: Direction,
        task: Task,
    ) where
        Task: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.tasks.spawn(task).id();
        let replaced = self.workers.insert(task_id, (worker_id, direction));
        debug_assert!(replaced.is_none(), "Tokio reused a live codec supervisor task ID");
    }

    pub(in crate::media::codec) fn observe_finished(&mut self) -> Vec<(WorkerId, Direction)> {
        let mut failed = Vec::new();
        while let Some(result) = self.tasks.try_join_next_with_id() {
            match result {
                Ok((task_id, ())) => {
                    self.workers.remove(&task_id);
                }
                Err(error) => {
                    let task_id = error.id();
                    if let Some((worker_id, direction)) = self.workers.remove(&task_id) {
                        tracing::error!(
                            worker_id,
                            ?direction,
                            %error,
                            "codec supervisor task terminated unexpectedly"
                        );
                        failed.push((worker_id, direction));
                    } else {
                        tracing::error!(%error, "untracked codec supervisor task terminated");
                    }
                }
            }
        }
        failed
    }

    pub(in crate::media::codec) fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Stops owning unfinished task handles without cancelling their futures.
    /// Those futures still own the native thread lifecycle and must be allowed
    /// to quarantine an in-flight native call after the service deadline.
    pub(in crate::media::codec) fn detach_all(&mut self) -> usize {
        let count = self.tasks.len();
        self.tasks.detach_all();
        self.workers.clear();
        count
    }
}
