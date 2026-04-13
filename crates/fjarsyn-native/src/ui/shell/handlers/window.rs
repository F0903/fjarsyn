use fjarsyn_core::executors::{AppEvent, LifecycleAction};
use iced::{Task, window as iced_window};

use crate::ui::{
    message::{Message, WindowControlMessage, WindowEventMessage},
    shell::{
        Fjarsyn,
        handlers::app_event,
        workflows::window::{self, WindowEffect},
    },
};

pub fn handle_window_event_msg(app: &mut Fjarsyn, message: WindowEventMessage) -> Task<Message> {
    let should_request_shutdown = matches!(
        message,
        WindowEventMessage::WindowClosed(id)
            if app
                .ctx
                .ui
                .main_window
                .as_ref()
                .map(|window| window.iced_id == id)
                .unwrap_or(false)
    );

    let effects = window::reduce_event(app, message);
    let lifecycle_task = if should_request_shutdown {
        app_event::execute_app_event(app, AppEvent::Lifecycle(LifecycleAction::ShutdownRequested))
    } else {
        Task::none()
    };

    Task::batch([lifecycle_task, Task::batch(effects.into_iter().map(run_effect))])
}

pub fn handle_window_control_msg(
    app: &mut Fjarsyn,
    message: WindowControlMessage,
) -> Task<Message> {
    let effects = window::reduce_control(app, message);
    Task::batch(effects.into_iter().map(run_effect))
}

fn run_effect(effect: WindowEffect) -> Task<Message> {
    match effect {
        WindowEffect::FetchRawId(id) => iced_window::raw_id::<Message>(id).map(move |raw_id| {
            Message::WindowEvent(WindowEventMessage::WindowRawIdFetched((id, raw_id)))
        }),
        WindowEffect::SyncMaximized(id) => iced_window::is_maximized(id)
            .map(|maximized| Message::WindowEvent(WindowEventMessage::WindowMaximized(maximized))),
        WindowEffect::Minimize(id) => iced_window::minimize(id, true),
        WindowEffect::ToggleMaximize(id) => iced_window::toggle_maximize(id),
        WindowEffect::Close(id) => iced_window::close(id),
        WindowEffect::Drag(id) => iced_window::drag(id),
        WindowEffect::Resize(id, direction) => iced_window::drag_resize(id, direction),
        WindowEffect::Exit => iced::exit(),
    }
}
