//! Window controls and observed window events.

#[derive(Debug, Clone)]
pub(in crate::ui) enum Control {
    Minimize,
    Maximize,
    Close,
    Drag,
    Resize(iced::window::Direction),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Event {
    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRawIdFetched((iced::window::Id, u64)),
    WindowMaximized(bool),
    SyncMaximized,
}
