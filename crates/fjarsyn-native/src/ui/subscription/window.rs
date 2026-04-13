use crate::ui::message::{Message, WindowEventMessage};

pub(super) fn map_window_event(event: iced::Event) -> Option<Message> {
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
