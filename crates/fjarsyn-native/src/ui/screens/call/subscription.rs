use futures::stream::unfold;
use iced::Subscription;

use super::{CallMessage, CallScreen, pipeline::LatestFrameReceiverRef};
use crate::ui::message::{Message, ScreenMessage};

type FrameSubscriptionStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Message> + Send + 'static>>;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum FrameSubscriptionKind {
    Local,
    Remote,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FrameSubscriptionData {
    receiver: LatestFrameReceiverRef,
    kind: FrameSubscriptionKind,
}

pub(super) fn build(screen: &CallScreen) -> Subscription<Message> {
    let mut subscriptions = vec![];

    if let Some(receiver) = screen.local.latest_frame_receiver() {
        subscriptions.push(latest_frame_subscription(FrameSubscriptionData {
            receiver,
            kind: FrameSubscriptionKind::Local,
        }));
    }

    if let Some(receiver) = screen.remote.decoded_frame_receiver() {
        subscriptions.push(latest_frame_subscription(FrameSubscriptionData {
            receiver,
            kind: FrameSubscriptionKind::Remote,
        }));
    }

    Subscription::batch(subscriptions)
}

fn latest_frame_subscription(data: FrameSubscriptionData) -> Subscription<Message> {
    Subscription::run_with(data, build_latest_frame_subscription)
}

fn build_latest_frame_subscription(data: &FrameSubscriptionData) -> FrameSubscriptionStream {
    let receiver = data.receiver.0.clone();
    let kind = data.kind;

    Box::pin(unfold(receiver, move |receiver| async move {
        loop {
            let result = {
                let mut lock = receiver.lock().await;
                lock.changed().await
            };

            match result {
                Ok(()) => {
                    let frame = {
                        let lock = receiver.lock().await;
                        lock.borrow().clone()
                    };

                    let message = match (kind, frame) {
                        (FrameSubscriptionKind::Local, Some(frame)) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::LocalFrameReady(frame)),
                        )),
                        (FrameSubscriptionKind::Local, None) => None,
                        (FrameSubscriptionKind::Remote, Some(frame)) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::DecodedFrameReady(frame)),
                        )),
                        (FrameSubscriptionKind::Remote, None) => Some(Message::Screen(
                            ScreenMessage::Call(CallMessage::DecodedFrameCleared),
                        )),
                    };

                    if let Some(message) = message {
                        return Some((message, receiver));
                    }
                }
                Err(_) => return None,
            }
        }
    }))
}
