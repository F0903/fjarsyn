use std::sync::Arc;

use bytes::Bytes;
use futures::stream::unfold;
use iced::Subscription;
use tokio::sync::{Mutex, mpsc};

use crate::{
    networking::discovery::DiscoveryEvent,
    services::call_service::CallEvent,
    ui::{
        app::Fjarsyn,
        message::{CallServiceMessage, Message, WindowEventMessage},
    },
};

// Wrapper to implement Hash which is needed by iced subscriptions.
#[derive(Clone)]
pub struct EventReceiverRef<T>(pub Arc<Mutex<mpsc::Receiver<T>>>);

impl<T> std::hash::Hash for EventReceiverRef<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl<T> PartialEq for EventReceiverRef<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl<T> Eq for EventReceiverRef<T> {}

pub fn subscription(app: &Fjarsyn) -> Subscription<Message> {
    use crate::ui::screens::Screen;

    let screen_subscriptions = app.active_screen.subscription(&app.ctx);

    let frame_subscription = packet_subscription(app.ctx.media.frame_packet_rx.0.clone());

    let call_event_subscription = app
        .ctx
        .networking
        .call_event_rx
        .as_ref()
        .map(|rx| call_event_subscription(rx.clone()))
        .unwrap_or(Subscription::none());

    let discovery_subscription = app
        .ctx
        .networking
        .discovery_event_rx
        .as_ref()
        .map(|rx| discovery_event_subscription(rx.clone()))
        .unwrap_or(Subscription::none());

    let window_open_subscription = iced::window::open_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)));
    let window_close_subscription = iced::window::close_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowClosed(id)));
    let window_event_subscription = iced::event::listen().filter_map(|event| match event {
        iced::Event::Window(iced::window::Event::Resized(_)) => {
            Some(Message::WindowEvent(WindowEventMessage::SyncMaximized))
        }
        _ => None,
    });
    let tick_subscription =
        iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);

    Subscription::batch(vec![
        screen_subscriptions,
        frame_subscription,
        call_event_subscription,
        discovery_subscription,
        window_open_subscription,
        window_close_subscription,
        window_event_subscription,
        tick_subscription,
    ])
}

pub fn call_event_subscription(
    receiver: Arc<Mutex<mpsc::Receiver<CallEvent>>>,
) -> Subscription<Message> {
    Subscription::run_with(EventReceiverRef(receiver), |receiver_ref| {
        let receiver = receiver_ref.0.clone();
        Box::new(Box::pin(unfold(
            receiver,
            |receiver: Arc<Mutex<mpsc::Receiver<CallEvent>>>| async move {
                let mut lock = receiver.lock().await;
                if let Some(event) = lock.recv().await {
                    drop(lock);
                    Some((Message::CallService(CallServiceMessage::CallEvent(event)), receiver))
                } else {
                    drop(lock);
                    None
                }
            },
        )))
    })
}

pub fn discovery_event_subscription(
    receiver: Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>,
) -> Subscription<Message> {
    Subscription::run_with(EventReceiverRef(receiver), |receiver_ref| {
        let receiver = receiver_ref.0.clone();
        Box::new(Box::pin(unfold(
            receiver,
            |receiver: Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>| async move {
                let mut lock = receiver.lock().await;
                if let Some(event) = lock.recv().await {
                    drop(lock);
                    Some((
                        Message::CallService(CallServiceMessage::DiscoveryEvent(event)),
                        receiver,
                    ))
                } else {
                    drop(lock);
                    None
                }
            },
        )))
    })
}

pub fn packet_subscription(receiver: Arc<Mutex<mpsc::Receiver<Bytes>>>) -> Subscription<Message> {
    Subscription::run_with(EventReceiverRef(receiver), |receiver_ref| {
        let receiver = receiver_ref.0.clone();
        Box::new(Box::pin(unfold(receiver, |receiver| async move {
            let mut lock = receiver.lock().await;
            if let Some(packet) = lock.recv().await {
                drop(lock);
                Some((Message::CallService(CallServiceMessage::PacketReceived(packet)), receiver))
            } else {
                drop(lock);
                None
            }
        })))
    })
}
