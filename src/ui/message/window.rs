#[derive(Debug, Clone)]
pub enum WindowEventMessage {
    WindowOpened(iced::window::Id),
    WindowClosed(iced::window::Id),
    WindowRawIdFetched((iced::window::Id, u64)),
    WindowMaximized(bool),
    CursorEntered,
    CursorLeft,
    SyncMaximized,
}

#[derive(Debug, Clone)]
pub enum WindowControlMessage {
    Minimize,
    Maximize,
    Close,
    Drag,
    Resize(iced::window::Direction),
}
