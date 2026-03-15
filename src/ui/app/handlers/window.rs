use iced::{Task, window as iced_window};

use crate::ui::{
    app::{Fjarsyn, WindowInfo},
    message::{Message, WindowControlMessage, WindowEventMessage},
};

pub fn handle_window_event_msg(app: &mut Fjarsyn, msg: WindowEventMessage) -> Task<Message> {
    match msg {
        WindowEventMessage::WindowOpened(id) => handle_window_opened(app, id),
        WindowEventMessage::WindowClosed(id) => handle_window_closed(app, id),
        WindowEventMessage::WindowMaximized(max) => {
            if let Some(w) = app.ctx.ui.main_window.as_mut() {
                w.maximized = max;
            }
            Task::none()
        }
        WindowEventMessage::SyncMaximized => app
            .ctx
            .ui
            .main_window
            .as_ref()
            .map(|w| {
                iced_window::is_maximized(w.iced_id)
                    .map(|m| Message::WindowEvent(WindowEventMessage::WindowMaximized(m)))
            })
            .unwrap_or(Task::none()),
        WindowEventMessage::WindowRawIdFetched((id, rid)) => handle_window_raw_id(app, id, rid),
    }
}

pub fn handle_window_control_msg(app: &mut Fjarsyn, msg: WindowControlMessage) -> Task<Message> {
    match msg {
        WindowControlMessage::Minimize => window_action(app, |id| iced_window::minimize(id, true)),
        WindowControlMessage::Maximize => window_action(app, iced_window::toggle_maximize),
        WindowControlMessage::Close => window_action(app, iced_window::close),
        WindowControlMessage::Drag => window_action(app, iced_window::drag),
        WindowControlMessage::Resize(dir) => {
            window_action(app, |id| iced_window::drag_resize(id, dir))
        }
    }
}

fn handle_window_opened(app: &mut Fjarsyn, id: iced_window::Id) -> Task<Message> {
    if app.ctx.ui.main_window.is_none() {
        app.ctx.ui.main_window = Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
    }
    iced_window::raw_id::<Message>(id)
        .map(move |rid| Message::WindowEvent(WindowEventMessage::WindowRawIdFetched((id, rid))))
}

fn handle_window_closed(app: &mut Fjarsyn, id: iced_window::Id) -> Task<Message> {
    if app.ctx.ui.main_window.as_ref().map(|w| w.iced_id == id).unwrap_or(false) {
        app.ctx.ui.main_window = None;
        return iced::exit();
    }
    Task::none()
}

fn handle_window_raw_id(app: &mut Fjarsyn, id: iced_window::Id, raw_id: u64) -> Task<Message> {
    if let Some(w) = app.ctx.ui.main_window.as_mut().filter(|w| w.iced_id == id) {
        w.raw_id = Some(raw_id);
    }
    Task::none()
}

fn window_action(app: &Fjarsyn, f: impl FnOnce(iced_window::Id) -> Task<Message>) -> Task<Message> {
    app.ctx.ui.main_window.as_ref().map(|w| f(w.iced_id)).unwrap_or(Task::none())
}
