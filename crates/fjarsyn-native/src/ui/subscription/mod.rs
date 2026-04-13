mod deadline;
mod events;
mod receiver;
mod window;

use iced::Subscription;
pub use receiver::EventReceiverRef;

use crate::ui::{
    app::{AppContext, Fjarsyn},
    message::{Message, WindowEventMessage},
};

pub fn subscription(app: &Fjarsyn) -> Subscription<Message> {
    use crate::ui::screens::Screen;

    let screen_subscriptions =
        app.active_screen.subscription(AppContext { state: &app.ctx, runtime: &app.runtime });

    let call_event_subscription =
        events::call_event_subscription(app.runtime.call_event_rx.clone());
    let discovery_subscription =
        events::discovery_event_subscription(app.runtime.discovery_event_rx.clone());
    let messaging_subscription =
        events::messaging_event_subscription(app.runtime.messaging_event_rx.clone());

    let window_open_subscription = iced::window::open_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)));
    let window_close_subscription = iced::window::close_events()
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowClosed(id)));
    let window_event_subscription = iced::event::listen().filter_map(window::map_window_event);
    let deadline_subscription = deadline::next_deadline(app)
        .map(|due_at| deadline::deadline_subscription(app.ctx.ui.started_at, due_at))
        .unwrap_or(Subscription::none());

    Subscription::batch(vec![
        screen_subscriptions,
        call_event_subscription,
        discovery_subscription,
        messaging_subscription,
        window_open_subscription,
        window_close_subscription,
        window_event_subscription,
        deadline_subscription,
    ])
}
