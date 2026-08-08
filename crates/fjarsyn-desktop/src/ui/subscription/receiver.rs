use std::sync::Arc;

use futures::stream::unfold;
use tokio::sync::{Mutex, mpsc};

use crate::ui::message::Message;

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

type ChannelSubscriptionStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Message> + Send + 'static>>;

#[derive(Clone)]
struct ChannelSubscriptionData<T> {
    receiver: Receiver<T>,
    map: fn(T) -> Message,
}

impl<T> std::hash::Hash for ChannelSubscriptionData<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.receiver.hash(state);
        (self.map as usize).hash(state);
    }
}

impl<T> PartialEq for ChannelSubscriptionData<T> {
    fn eq(&self, other: &Self) -> bool {
        self.receiver == other.receiver && std::ptr::fn_addr_eq(self.map, other.map)
    }
}

impl<T> Eq for ChannelSubscriptionData<T> {}

pub(super) fn channel_subscription<T: Send + 'static>(
    receiver: Receiver<T>,
    map: fn(T) -> Message,
) -> iced::Subscription<Message> {
    iced::Subscription::run_with(
        ChannelSubscriptionData { receiver, map },
        build_channel_subscription::<T>,
    )
}

fn build_channel_subscription<T: Send + 'static>(
    data: &ChannelSubscriptionData<T>,
) -> ChannelSubscriptionStream {
    let receiver = data.receiver.0.clone();
    let map = data.map;

    Box::pin(unfold(receiver, move |receiver| async move {
        let mut lock = receiver.lock().await;
        if let Some(event) = lock.recv().await {
            drop(lock);
            Some((map(event), receiver))
        } else {
            drop(lock);
            None
        }
    }))
}
