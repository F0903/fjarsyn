//! Shared codec registry, health, and shutdown state.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::{Mutex as AsyncMutex, watch};

use super::{
    Config, Direction, DirectionState, Error, Health, Operation, Poison, PoisonReason,
    backend::BackendFactory,
    registry::{WorkerDirective, WorkerId, WorkerReservation},
    worker::SupervisorTasks,
};

struct WorkerRecord {
    direction: Direction,
    directive: watch::Sender<WorkerDirective>,
    accepting: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
}

struct StateData {
    shutting_down: bool,
    next_worker_id: WorkerId,
    snapshot: Health,
    workers: BTreeMap<WorkerId, WorkerRecord>,
    quarantined_workers: usize,
}

pub(in crate::media::codec) struct State {
    config: Config,
    data: Mutex<StateData>,
    snapshot_tx: watch::Sender<Health>,
    backend: Arc<dyn BackendFactory>,
    supervisor_tasks: AsyncMutex<SupervisorTasks>,
}

impl State {
    pub(in crate::media::codec) fn new(
        config: Config,
        backend: Arc<dyn BackendFactory>,
    ) -> Arc<Self> {
        let snapshot = Health::default();
        let (snapshot_tx, _) = watch::channel(snapshot.clone());
        Arc::new(Self {
            config,
            data: Mutex::new(StateData {
                shutting_down: false,
                next_worker_id: 1,
                snapshot,
                workers: BTreeMap::new(),
                quarantined_workers: 0,
            }),
            snapshot_tx,
            backend,
            supervisor_tasks: AsyncMutex::new(SupervisorTasks::new()),
        })
    }

    pub(in crate::media::codec) async fn spawn_supervisor<Task>(
        &self,
        worker_id: WorkerId,
        direction: Direction,
        task: Task,
    ) where
        Task: Future<Output = ()> + Send + 'static,
    {
        let failed = {
            let mut supervisors = self.supervisor_tasks.lock().await;
            let failed = supervisors.observe_finished();
            supervisors.spawn(worker_id, direction, task);
            failed
        };
        self.handle_failed_supervisors(failed);
    }

    pub(in crate::media::codec) fn try_observe_supervisor_tasks(&self) -> Option<usize> {
        let (failed, remaining) = {
            let mut supervisors = self.supervisor_tasks.try_lock().ok()?;
            let failed = supervisors.observe_finished();
            (failed, supervisors.len())
        };
        self.handle_failed_supervisors(failed);
        Some(remaining)
    }

    pub(in crate::media::codec) fn try_detach_supervisor_tasks(&self) -> Option<usize> {
        let (failed, detached) = {
            let mut supervisors = self.supervisor_tasks.try_lock().ok()?;
            let failed = supervisors.observe_finished();
            let detached = supervisors.detach_all();
            (failed, detached)
        };
        self.handle_failed_supervisors(failed);
        Some(detached)
    }

    fn handle_failed_supervisors(&self, failed: Vec<(WorkerId, Direction)>) {
        for (worker_id, direction) in failed {
            self.poison(direction, Operation::Shutdown, PoisonReason::WorkerTerminated);
            self.remove_worker(worker_id);
        }
    }

    pub(in crate::media::codec) fn call_timeout(&self) -> Duration {
        self.config.call_timeout
    }

    pub(in crate::media::codec) fn stop_timeout(&self) -> Duration {
        self.config.stop_timeout
    }

    pub(in crate::media::codec) fn backend(&self) -> Arc<dyn BackendFactory> {
        self.backend.clone()
    }

    pub(in crate::media::codec) fn snapshot(&self) -> Health {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot.clone()
    }

    pub(in crate::media::codec) fn subscribe(&self) -> watch::Receiver<Health> {
        self.snapshot_tx.subscribe()
    }

    pub(in crate::media::codec) fn reserve_worker(
        &self,
        direction: Direction,
    ) -> Result<WorkerReservation, Error> {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.shutting_down {
            return Err(Error::ShuttingDown);
        }
        if let DirectionState::RestartRequired(poison) = data.snapshot.direction(direction) {
            return Err(Error::RestartRequired(poison.clone()));
        }

        let id = data.next_worker_id;
        data.next_worker_id = data.next_worker_id.saturating_add(1);
        let (directive, receiver) = watch::channel(WorkerDirective::Run);
        let accepting = Arc::new(AtomicBool::new(true));
        let publishing = Arc::new(AtomicBool::new(true));
        data.workers.insert(
            id,
            WorkerRecord {
                direction,
                directive,
                accepting: accepting.clone(),
                publishing: publishing.clone(),
            },
        );
        Ok(WorkerReservation::new(id, receiver, accepting, publishing))
    }

    pub(in crate::media::codec) fn remove_worker(&self, id: WorkerId) {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).workers.remove(&id);
    }

    pub(in crate::media::codec) fn quarantine_worker(&self, id: WorkerId) -> bool {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.workers.remove(&id).is_none() {
            return false;
        }
        data.quarantined_workers = data.quarantined_workers.saturating_add(1);
        true
    }

    pub(in crate::media::codec) fn release_quarantined_worker(&self) {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        data.quarantined_workers = data.quarantined_workers.saturating_sub(1);
    }

    pub(in crate::media::codec) fn request_stop(&self, id: WorkerId) {
        let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(worker) = data.workers.get(&id)
            && matches!(*worker.directive.borrow(), WorkerDirective::Run)
        {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            worker.directive.send_replace(WorkerDirective::Stop);
        }
    }

    pub(in crate::media::codec) fn poison(
        &self,
        direction: Direction,
        operation: Operation,
        reason: PoisonReason,
    ) -> Poison {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing = match data.snapshot.direction(direction) {
            DirectionState::Available => None,
            DirectionState::RestartRequired(poison) => Some(poison.clone()),
        };
        if let Some(existing) = existing {
            return existing;
        }

        let poison = Poison { direction, operation, reason };
        match direction {
            Direction::Encode => {
                data.snapshot.encode = DirectionState::RestartRequired(poison.clone());
            }
            Direction::Decode => {
                data.snapshot.decode = DirectionState::RestartRequired(poison.clone());
            }
        }
        for worker in data.workers.values().filter(|worker| worker.direction == direction) {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            worker.directive.send_replace(WorkerDirective::Poisoned(poison.clone()));
        }
        self.snapshot_tx.send_replace(data.snapshot.clone());
        poison
    }

    pub(in crate::media::codec) fn begin_shutdown(&self) {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.shutting_down {
            return;
        }
        data.shutting_down = true;
        for worker in data.workers.values() {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            if !matches!(*worker.directive.borrow(), WorkerDirective::Poisoned(_)) {
                worker.directive.send_replace(WorkerDirective::ServiceShutdown);
            }
        }
    }

    pub(in crate::media::codec) fn poison_unfinished_shutdowns(&self) {
        let directions = {
            let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            data.workers.values().map(|worker| worker.direction).collect::<Vec<_>>()
        };
        for direction in directions {
            self.poison(direction, Operation::Shutdown, PoisonReason::DeadlineExceeded);
        }
    }

    pub(in crate::media::codec) fn shutdown_counts(&self) -> (usize, usize) {
        let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (data.workers.len(), data.quarantined_workers)
    }

    pub(in crate::media::codec) fn detach_unfinished_workers(&self) {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).workers.clear();
    }
}
