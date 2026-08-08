//! Pure window-event reduction and the resulting Iced effects.

use iced::window;

use crate::ui::{
    message,
    shell::{UiState, WindowInfo},
};

pub(in crate::ui::shell) enum Effect {
    FetchRawId(window::Id),
    SyncMaximized(window::Id),
    Minimize(window::Id),
    ToggleMaximize(window::Id),
    Close(window::Id),
    Drag(window::Id),
    Resize(window::Id, window::Direction),
}

// Window workflows keep UI state mutations and platform-window side effects
// separate. The reducer updates state and describes which runtime action should
// happen next.
pub(in crate::ui::shell) fn reduce_event(
    ui: &mut UiState,
    message: message::window::Event,
) -> Vec<Effect> {
    match message {
        message::window::Event::WindowOpened(id) => {
            if ui.main_window.is_none() {
                ui.main_window = Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
            }
            vec![Effect::FetchRawId(id)]
        }
        message::window::Event::WindowClosed(id) => {
            if ui.main_window.as_ref().is_some_and(|window| window.iced_id == id) {
                ui.main_window = None;
            }
            Vec::new()
        }
        message::window::Event::WindowRawIdFetched((id, raw_id)) => {
            if let Some(window) = ui.main_window.as_mut().filter(|window| window.iced_id == id) {
                window.raw_id = Some(raw_id);
            }
            Vec::new()
        }
        message::window::Event::WindowMaximized(maximized) => {
            if let Some(window) = ui.main_window.as_mut() {
                window.maximized = maximized;
            }
            Vec::new()
        }
        message::window::Event::SyncMaximized => ui
            .main_window
            .as_ref()
            .map(|window| vec![Effect::SyncMaximized(window.iced_id)])
            .unwrap_or_default(),
    }
}

pub(in crate::ui::shell) fn reduce_control(
    ui: &UiState,
    message: message::window::Control,
) -> Vec<Effect> {
    let Some(window_id) = ui.main_window.as_ref().map(|window| window.iced_id) else {
        return Vec::new();
    };

    match message {
        message::window::Control::Minimize => vec![Effect::Minimize(window_id)],
        message::window::Control::Maximize => vec![Effect::ToggleMaximize(window_id)],
        message::window::Control::Close => vec![Effect::Close(window_id)],
        message::window::Control::Drag => vec![Effect::Drag(window_id)],
        message::window::Control::Resize(direction) => {
            vec![Effect::Resize(window_id, direction)]
        }
    }
}
