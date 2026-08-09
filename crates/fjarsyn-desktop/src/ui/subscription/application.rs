use std::time::Instant;

use iced::Subscription;

use super::{deadline, receiver};
use crate::ui::{
    message::{self, Message},
    runtime,
};

pub(in crate::ui) fn subscription(
    engine_receivers: Option<runtime::EngineReceivers>,
    started_at: Instant,
    notification_deadline: Option<Instant>,
) -> Subscription<Message> {
    let runtime = engine_receivers
        .map(|receivers| {
            let runtime_id = receivers.runtime_id;
            Subscription::batch([
                receiver::retained_subscription(
                    runtime_id,
                    receivers.state,
                    map_engine_state_changed,
                ),
                receiver::channel_subscription(runtime_id, receivers.notices, map_engine_notice),
                receiver::channel_subscription(
                    runtime_id,
                    receivers.failures,
                    map_engine_adapter_failure,
                ),
            ])
        })
        .unwrap_or_else(Subscription::none);
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

fn map_engine_state_changed(runtime_id: runtime::RuntimeId) -> Message {
    Message::Runtime(message::Runtime::EngineStateChanged { runtime_id })
}

fn map_engine_notice(runtime_id: runtime::RuntimeId, notice: runtime::EngineNotice) -> Message {
    Message::Runtime(message::Runtime::EngineNotice { runtime_id, notice })
}

fn map_engine_adapter_failure(
    runtime_id: runtime::RuntimeId,
    failure: runtime::EngineAdapterFailure,
) -> Message {
    Message::Runtime(message::Runtime::EngineAdapterFailed { runtime_id, failure })
}

fn map_window_event(event: iced::Event) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::Resized(_)) => {
            Some(Message::WindowEvent(message::window::Event::SyncMaximized))
        }
        _ => None,
    }
}
