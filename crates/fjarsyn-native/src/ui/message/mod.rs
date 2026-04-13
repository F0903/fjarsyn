mod call;
mod capture;
mod config;
mod database;
mod lifecycle;
mod navigation;
mod notification;
mod screen;
mod service;
mod window;

pub use call::{CallActionMessage, CallTarget};
pub use capture::CaptureMessage;
pub use config::ConfigMessage;
pub use database::DatabaseMessage;
pub use lifecycle::LifecycleMessage;
pub use navigation::{NavigationMessage, Route};
pub use notification::NotificationMessage;
pub use screen::ScreenMessage;
pub use service::{CallServiceMessage, ContactsServiceMessage, MessagingServiceMessage};
pub use window::{WindowControlMessage, WindowEventMessage};

#[derive(Debug, Clone)]
pub enum Message {
    Navigation(NavigationMessage),
    Lifecycle(LifecycleMessage),
    Config(ConfigMessage),
    Screen(ScreenMessage),
    CallAction(CallActionMessage),
    CallService(CallServiceMessage),
    Notification(NotificationMessage),
    Database(DatabaseMessage),
    ContactData(ContactsServiceMessage),
    Messaging(MessagingServiceMessage),
    Capture(CaptureMessage),
    WindowEvent(WindowEventMessage),
    WindowControl(WindowControlMessage),

    CopyId(String),
    Tick(std::time::Instant),
    Batch(Vec<Message>),
    NoOp,
}
