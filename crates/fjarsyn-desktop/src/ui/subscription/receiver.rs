use std::sync::Arc;

use futures::stream::unfold;
use tokio::sync::{Mutex, mpsc};

use crate::ui::{
    message::Message,
    runtime::{Retained, RuntimeId},
};

/// Shared receiver identity suitable for use as an iced subscription key.
#[derive(Clone)]
pub(in crate::ui) struct Receiver<T>(Arc<Mutex<mpsc::Receiver<T>>>);

impl<T> Receiver<T> {
    pub(in crate::ui) fn new(receiver: mpsc::Receiver<T>) -> Self {
        Self(Arc::new(Mutex::new(receiver)))
    }
}

impl<T> std::hash::Hash for Receiver<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl<T> PartialEq for Receiver<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for Receiver<T> {}

type SubscriptionStream = std::pin::Pin<Box<dyn futures::Stream<Item = Message> + Send + 'static>>;

#[derive(Clone)]
struct ChannelSubscriptionData<T> {
    runtime_id: RuntimeId,
    receiver: Receiver<T>,
    map: fn(RuntimeId, T) -> Message,
}

impl<T> std::hash::Hash for ChannelSubscriptionData<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.runtime_id.hash(state);
        self.receiver.hash(state);
        (self.map as usize).hash(state);
    }
}

impl<T> PartialEq for ChannelSubscriptionData<T> {
    fn eq(&self, other: &Self) -> bool {
        self.runtime_id == other.runtime_id
            && self.receiver == other.receiver
            && std::ptr::fn_addr_eq(self.map, other.map)
    }
}

impl<T> Eq for ChannelSubscriptionData<T> {}

pub(super) fn channel_subscription<T: Send + 'static>(
    runtime_id: RuntimeId,
    receiver: Receiver<T>,
    map: fn(RuntimeId, T) -> Message,
) -> iced::Subscription<Message> {
    iced::Subscription::run_with(
        ChannelSubscriptionData { runtime_id, receiver, map },
        build_channel_subscription::<T>,
    )
}

fn build_channel_subscription<T: Send + 'static>(
    data: &ChannelSubscriptionData<T>,
) -> SubscriptionStream {
    let receiver = data.receiver.0.clone();
    let runtime_id = data.runtime_id;
    let map = data.map;

    Box::pin(unfold(receiver, move |receiver| async move {
        let mut lock = receiver.lock().await;
        if let Some(event) = lock.recv().await {
            drop(lock);
            Some((map(runtime_id, event), receiver))
        } else {
            drop(lock);
            None
        }
    }))
}

#[derive(Clone)]
struct RetainedSubscriptionData<T> {
    runtime_id: RuntimeId,
    retained: Retained<T>,
    map: fn(RuntimeId) -> Message,
}

impl<T> std::hash::Hash for RetainedSubscriptionData<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.runtime_id.hash(state);
        (self.map as usize).hash(state);
    }
}

impl<T> PartialEq for RetainedSubscriptionData<T> {
    fn eq(&self, other: &Self) -> bool {
        self.runtime_id == other.runtime_id && std::ptr::fn_addr_eq(self.map, other.map)
    }
}

impl<T> Eq for RetainedSubscriptionData<T> {}

/// Emits a lightweight wake whenever a retained watch value changes.
///
/// Iced may already have an older wake queued. That is intentional: wakes
/// carry no state, and every accepted wake reads the newest retained value.
pub(super) fn retained_subscription<T: Send + Sync + 'static>(
    runtime_id: RuntimeId,
    retained: Retained<T>,
    map: fn(RuntimeId) -> Message,
) -> iced::Subscription<Message> {
    iced::Subscription::run_with(
        RetainedSubscriptionData { runtime_id, retained, map },
        build_retained_subscription::<T>,
    )
}

fn build_retained_subscription<T: Send + Sync + 'static>(
    data: &RetainedSubscriptionData<T>,
) -> SubscriptionStream {
    let receiver = data.retained.subscribe();
    let runtime_id = data.runtime_id;
    let map = data.map;

    Box::pin(unfold(receiver, move |mut receiver| async move {
        receiver.changed().await.ok()?;
        Some((map(runtime_id), receiver))
    }))
}
