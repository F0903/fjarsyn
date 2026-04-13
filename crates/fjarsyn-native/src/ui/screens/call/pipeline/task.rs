use std::sync::Mutex as StdMutex;

use tokio::task::JoinHandle;

pub(super) struct AbortOnDropTask {
    task: StdMutex<Option<JoinHandle<()>>>,
}

impl AbortOnDropTask {
    pub(super) fn new(task: JoinHandle<()>) -> Self {
        Self { task: StdMutex::new(Some(task)) }
    }
}

impl Drop for AbortOnDropTask {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}
