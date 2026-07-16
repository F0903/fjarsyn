mod config;
mod lifecycle;
mod navigation;
mod notification;
mod peer;
mod screen;
mod service;
mod window;

pub use config::ConfigMessage;
pub use lifecycle::LifecycleMessage;
pub use navigation::{NavigationMessage, Route};
pub use notification::NotificationMessage;
pub use peer::{MessageSendOutcome, PeerActionMessage};
pub use screen::ScreenMessage;
pub use service::{ContactOperationId, ContactsServiceMessage, RuntimeMessage};
pub use window::{WindowControlMessage, WindowEventMessage};

#[derive(Debug, Clone)]
pub enum Message {
    Navigation(NavigationMessage),
    Lifecycle(LifecycleMessage),
    Config(ConfigMessage),
    Screen(ScreenMessage),
    PeerAction(PeerActionMessage),
    Runtime(RuntimeMessage),
    Notification(NotificationMessage),
    ContactData(ContactsServiceMessage),
    WindowEvent(WindowEventMessage),
    WindowControl(WindowControlMessage),

    CopyId(String),
    CopyInvite(String),
    CopyFingerprint(String),
    Tick(std::time::Instant),
    NoOp,
}
