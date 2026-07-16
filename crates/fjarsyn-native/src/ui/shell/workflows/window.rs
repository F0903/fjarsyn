use iced::window as iced_window;

use crate::ui::{
    message::{WindowControlMessage, WindowEventMessage},
    shell::{Fjarsyn, WindowInfo},
};

pub(crate) enum WindowEffect {
    FetchRawId(iced_window::Id),
    SyncMaximized(iced_window::Id),
    Minimize(iced_window::Id),
    ToggleMaximize(iced_window::Id),
    Close(iced_window::Id),
    Drag(iced_window::Id),
    Resize(iced_window::Id, iced_window::Direction),
}

// Window workflows keep UI state mutations and platform-window side effects
// separate. The reducer updates state and describes which runtime action should
// happen next.
pub(crate) fn reduce_event(app: &mut Fjarsyn, message: WindowEventMessage) -> Vec<WindowEffect> {
    match message {
        WindowEventMessage::WindowOpened(id) => {
            if app.ctx.ui.main_window.is_none() {
                app.ctx.ui.main_window =
                    Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
            }
            vec![WindowEffect::FetchRawId(id)]
        }
        WindowEventMessage::WindowClosed(id) => {
            if app.ctx.ui.main_window.as_ref().map(|window| window.iced_id == id).unwrap_or(false) {
                app.ctx.ui.main_window = None;
                Vec::new()
            } else {
                Vec::new()
            }
        }
        WindowEventMessage::WindowRawIdFetched((id, raw_id)) => {
            if let Some(window) =
                app.ctx.ui.main_window.as_mut().filter(|window| window.iced_id == id)
            {
                window.raw_id = Some(raw_id);
            }
            Vec::new()
        }
        WindowEventMessage::WindowMaximized(maximized) => {
            if let Some(window) = app.ctx.ui.main_window.as_mut() {
                window.maximized = maximized;
            }
            Vec::new()
        }
        WindowEventMessage::CursorEntered => {
            app.ctx.ui.cursor_inside_window = true;
            Vec::new()
        }
        WindowEventMessage::CursorLeft => {
            app.ctx.ui.cursor_inside_window = false;
            Vec::new()
        }
        WindowEventMessage::SyncMaximized => app
            .ctx
            .ui
            .main_window
            .as_ref()
            .map(|window| vec![WindowEffect::SyncMaximized(window.iced_id)])
            .unwrap_or_default(),
    }
}

pub(crate) fn reduce_control(app: &Fjarsyn, message: WindowControlMessage) -> Vec<WindowEffect> {
    let Some(window_id) = app.ctx.ui.main_window.as_ref().map(|window| window.iced_id) else {
        return Vec::new();
    };

    match message {
        WindowControlMessage::Minimize => vec![WindowEffect::Minimize(window_id)],
        WindowControlMessage::Maximize => vec![WindowEffect::ToggleMaximize(window_id)],
        WindowControlMessage::Close => vec![WindowEffect::Close(window_id)],
        WindowControlMessage::Drag => vec![WindowEffect::Drag(window_id)],
        WindowControlMessage::Resize(direction) => vec![WindowEffect::Resize(window_id, direction)],
    }
}
