use iced::Task;

use crate::ui::{
    message::{self, Message},
    shell::{Fjarsyn, handlers::shutdown, window_workflow},
};

pub(in crate::ui::shell) fn handle_window_event_msg(
    app: &mut Fjarsyn,
    message: message::window::Event,
) -> Task<Message> {
    let closing_main = matches!(
        message,
        message::window::Event::WindowClosed(id)
            if app.state.ui.main_window.as_ref().is_some_and(|window| window.iced_id == id)
    );
    let effects = window_workflow::reduce_event(&mut app.state.ui, message);
    let window_tasks = Task::batch(effects.into_iter().map(run_effect));
    if closing_main { Task::batch([window_tasks, shutdown(app)]) } else { window_tasks }
}

pub(in crate::ui::shell) fn handle_window_control_msg(
    app: &mut Fjarsyn,
    message: message::window::Control,
) -> Task<Message> {
    Task::batch(window_workflow::reduce_control(&app.state.ui, message).into_iter().map(run_effect))
}

fn run_effect(effect: window_workflow::Effect) -> Task<Message> {
    match effect {
        window_workflow::Effect::FetchRawId(id) => {
            iced::window::raw_id::<Message>(id).map(move |raw_id| {
                Message::WindowEvent(message::window::Event::WindowRawIdFetched((id, raw_id)))
            })
        }
        window_workflow::Effect::SyncMaximized(id) => {
            iced::window::is_maximized(id).map(|maximized| {
                Message::WindowEvent(message::window::Event::WindowMaximized(maximized))
            })
        }
        window_workflow::Effect::Minimize(id) => iced::window::minimize(id, true),
        window_workflow::Effect::ToggleMaximize(id) => iced::window::toggle_maximize(id),
        window_workflow::Effect::Close(id) => iced::window::close(id),
        window_workflow::Effect::Drag(id) => iced::window::drag(id),
        window_workflow::Effect::Resize(id, direction) => iced::window::drag_resize(id, direction),
    }
}
