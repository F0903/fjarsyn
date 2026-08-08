use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
pub(in crate::media::codec::tests) struct BlockingGate {
    started: Arc<AtomicBool>,
    released: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingGate {
    pub(in crate::media::codec::tests) fn new() -> Self {
        Self {
            started: Arc::new(AtomicBool::new(false)),
            released: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub(in crate::media::codec::tests) fn block(&self) {
        self.started.store(true, Ordering::Release);
        let (released, wake) = &*self.released;
        let mut released = released.lock().unwrap();
        while !*released {
            released = wake.wait(released).unwrap();
        }
    }

    pub(in crate::media::codec::tests) fn release(&self) {
        let (released, wake) = &*self.released;
        *released.lock().unwrap() = true;
        wake.notify_all();
    }

    pub(in crate::media::codec::tests) async fn wait_until_started(&self) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !self.started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("scripted codec call did not start");
    }
}

pub(in crate::media::codec::tests) struct ReleaseGateOnDrop(
    pub(in crate::media::codec::tests) BlockingGate,
);

impl Drop for ReleaseGateOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
