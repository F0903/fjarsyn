use std::sync::Arc;

use futures::stream::{once, unfold};
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

type ChannelSubscriptionStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Message> + Send + 'static>>;

#[derive(Clone)]
struct ChannelSubscriptionData<T> {
    receiver: EventReceiverRef<T>,
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

#[derive(Clone, Copy)]
struct DeadlineSubData {
    deadline: std::time::Instant,
    since_start: std::time::Duration,
}

impl std::hash::Hash for DeadlineSubData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.since_start.hash(state);
    }
}

impl PartialEq for DeadlineSubData {
    fn eq(&self, other: &Self) -> bool {
        self.since_start == other.since_start
    }
}

impl Eq for DeadlineSubData {}

pub fn subscription(app: &Fjarsyn) -> Subscription<Message> {
    use crate::ui::screens::Screen;

    let screen_subscriptions = app.active_screen.subscription(&app.ctx);

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
    let window_event_subscription = iced::event::listen().filter_map(map_window_event);
    let deadline_subscription = next_deadline(app)
        .map(|deadline| deadline_subscription(app.ctx.ui.started_at, deadline))
        .unwrap_or(Subscription::none());

    Subscription::batch(vec![
        screen_subscriptions,
        call_event_subscription,
        discovery_subscription,
        window_open_subscription,
        window_close_subscription,
        window_event_subscription,
        deadline_subscription,
    ])
}

fn map_window_event(event: iced::Event) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::Resized(_)) => {
            Some(Message::WindowEvent(WindowEventMessage::SyncMaximized))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorEntered) => {
            Some(Message::WindowEvent(WindowEventMessage::CursorEntered))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
            Some(Message::WindowEvent(WindowEventMessage::CursorLeft))
        }
        _ => None,
    }
}

fn next_deadline(app: &Fjarsyn) -> Option<std::time::Instant> {
    match (app.ctx.ui.notifications.next_deadline(), app.ctx.session.incoming_call_timeout) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    }
}

fn deadline_subscription(
    started_at: std::time::Instant,
    deadline: std::time::Instant,
) -> Subscription<Message> {
    Subscription::run_with(
        DeadlineSubData { deadline, since_start: deadline.saturating_duration_since(started_at) },
        |data| {
            let deadline = data.deadline;
            once(async move {
                let deadline = tokio::time::Instant::from_std(deadline);
                let now = tokio::time::Instant::now();

                if deadline > now {
                    tokio::time::sleep_until(deadline).await;
                }

                Message::Tick(deadline.into_std())
            })
        },
    )
}

pub fn call_event_subscription(
    receiver: Arc<Mutex<mpsc::Receiver<CallEvent>>>,
) -> Subscription<Message> {
    channel_subscription(receiver, map_call_event)
}

pub fn discovery_event_subscription(
    receiver: Arc<Mutex<mpsc::Receiver<DiscoveryEvent>>>,
) -> Subscription<Message> {
    channel_subscription(receiver, map_discovery_event)
}

fn map_call_event(event: CallEvent) -> Message {
    Message::CallService(CallServiceMessage::CallEvent(event))
}

fn map_discovery_event(event: DiscoveryEvent) -> Message {
    Message::CallService(CallServiceMessage::DiscoveryEvent(event))
}

fn channel_subscription<T: Send + 'static>(
    receiver: Arc<Mutex<mpsc::Receiver<T>>>,
    map: fn(T) -> Message,
) -> Subscription<Message> {
    // Most event channels follow the same pattern: wait for the next item and
    // translate it into a UI message. Keep that adapter generic and leave only
    // the event-to-message mapping at the call site.
    Subscription::run_with(
        ChannelSubscriptionData { receiver: EventReceiverRef(receiver), map },
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
