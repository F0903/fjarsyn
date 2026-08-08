use std::time::Instant;

use iced::Subscription;

use super::{deadline, receiver};
use crate::ui::{
    message::{self, Message},
    runtime,
};

pub(in crate::ui) fn subscription(
    runtime_events: super::Receiver<runtime::Event>,
    started_at: Instant,
    notification_deadline: Option<Instant>,
) -> Subscription<Message> {
    let runtime = receiver::channel_subscription(runtime_events, map_runtime_event);
    let opened = iced::window::open_events()
        .map(|id| Message::WindowEvent(message::window::Event::WindowOpened(id)));
    let closed = iced::window::close_events()
        .map(|id| Message::WindowEvent(message::window::Event::WindowClosed(id)));
    let window = iced::event::listen().filter_map(map_window_event);
    let deadline = notification_deadline
        .map(|due_at| deadline::deadline_subscription(started_at, due_at))
        .unwrap_or_else(Subscription::none);

    Subscription::batch([runtime, opened, closed, window, deadline])
}

fn map_runtime_event(event: runtime::Event) -> Message {
    Message::Runtime(message::Runtime::Event(event))
}

fn map_window_event(event: iced::Event) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::Resized(_)) => {
            Some(Message::WindowEvent(message::window::Event::SyncMaximized))
        }
        _ => None,
    }
}
