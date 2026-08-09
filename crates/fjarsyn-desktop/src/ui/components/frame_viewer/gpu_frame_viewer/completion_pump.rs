use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use fjarsyn_engine::media::gpu_interop::ImportedFrameDrawGuard;

const POLL_INTERVAL: Duration = Duration::from_millis(8);

/// Polls wgpu only while imported draws have outstanding completion callbacks.
///
/// Queue submission normally dispatches these callbacks. The producer can stop
/// publishing once its bounded texture pool is full, though, so it cannot be
/// the only mechanism that advances completion.
pub(super) struct CompletionPump {
    activity: Arc<Activity>,
    worker: Option<thread::JoinHandle<()>>,
}

impl CompletionPump {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let activity = Arc::new(Activity::default());
        let worker_activity = activity.clone();
        let device = device.clone();
        let worker = thread::Builder::new()
            .name(String::from("fjarsyn-wgpu-completion"))
            .spawn(move || run(device, &worker_activity))
            .expect("failed to start the wgpu completion pump");

        Self { activity, worker: Some(worker) }
    }

    /// Retains a producer slot until the command buffer containing this render
    /// pass has actually completed on the GPU.
    pub(super) fn retain_until_submitted_work_done(
        &self,
        render_pass: &wgpu::RenderPass<'_>,
        guard: ImportedFrameDrawGuard,
    ) {
        let pending_draw = PendingDraw::new(guard, self.activity.clone());
        render_pass.on_submitted_work_done(move || drop(pending_draw));
    }
}

impl Drop for CompletionPump {
    fn drop(&mut self) {
        self.activity.owner_dropped();
        let Some(worker) = self.worker.take() else {
            return;
        };

        // Polling is non-blocking and the worker observes shutdown during its
        // bounded interval. Pending callbacks still retain their draw guards
        // until the surrounding encoder or device submits or discards them.
        if worker.thread().id() != thread::current().id() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct Activity {
    state: Mutex<ActivityState>,
    changed: Condvar,
}

struct ActivityState {
    pending: usize,
    owner_alive: bool,
}

impl Default for ActivityState {
    fn default() -> Self {
        Self { pending: 0, owner_alive: true }
    }
}

impl Activity {
    fn register(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.pending = state.pending.checked_add(1).expect("GPU draw count overflowed");
        self.changed.notify_one();
    }

    fn complete(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        debug_assert!(state.pending > 0);
        state.pending = state.pending.saturating_sub(1);
        self.changed.notify_one();
    }

    fn owner_dropped(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.owner_alive = false;
        self.changed.notify_one();
    }

    fn wait_for_work(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.pending == 0 && state.owner_alive {
            state = self.changed.wait(state).unwrap_or_else(|error| error.into_inner());
        }
        state.owner_alive && state.pending > 0
    }

    fn wait_for_next_poll(&self) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.pending == 0 {
            return;
        }

        let _ = self
            .changed
            .wait_timeout_while(state, POLL_INTERVAL, |state| {
                state.owner_alive && state.pending > 0
            })
            .unwrap_or_else(|error| error.into_inner());
    }
}

struct PendingDraw {
    guard: Option<ImportedFrameDrawGuard>,
    activity: Arc<Activity>,
}

impl PendingDraw {
    fn new(guard: ImportedFrameDrawGuard, activity: Arc<Activity>) -> Self {
        activity.register();
        Self { guard: Some(guard), activity }
    }
}

impl Drop for PendingDraw {
    fn drop(&mut self) {
        // Release the producer slot before making the pump idle.
        drop(self.guard.take());
        self.activity.complete();
    }
}

fn run(device: wgpu::Device, activity: &Activity) {
    while activity.wait_for_work() {
        if let Err(error) = device.poll(wgpu::PollType::Poll) {
            tracing::error!(%error, "Failed to poll for GPU frame completion");
            break;
        }
        activity.wait_for_next_poll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_the_owner_stops_polling_with_registered_work() {
        let activity = Activity::default();
        activity.register();
        assert!(activity.wait_for_work());

        activity.owner_dropped();
        assert!(!activity.wait_for_work());

        activity.complete();
        assert!(!activity.wait_for_work());
    }
}
