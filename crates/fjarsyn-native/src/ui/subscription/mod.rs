mod deadline;
mod receiver;
mod window;

use iced::Subscription;
pub use receiver::EventReceiverRef;

use crate::ui::{
    message::{Message, RuntimeMessage, WindowEventMessage},
    runtime::RuntimeEvent,
    shell::{Fjarsyn, ShellContext},
};

pub fn subscription(app: &Fjarsyn) -> Subscription<Message> {
    use crate::ui::screens::Screen;

    let screen = app.active_screen.subscription(ShellContext::new(&app.ctx));
    let runtime = receiver::channel_subscription(app.runtime.event_rx.clone(), map_runtime_event);
    let opened = iced::window::open_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)));
    let closed = iced::window::close_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowClosed(id)));
    let window = iced::event::listen().filter_map(window::map_window_event);
    let deadline = app
        .ctx
        .ui
        .notifications
        .next_deadline()
        .map(|due_at| deadline::deadline_subscription(app.ctx.ui.started_at, due_at))
        .unwrap_or_else(Subscription::none);

    Subscription::batch([screen, runtime, opened, closed, window, deadline])
}

fn map_runtime_event(event: RuntimeEvent) -> Message {
    Message::Runtime(RuntimeMessage::Event(event))
}
