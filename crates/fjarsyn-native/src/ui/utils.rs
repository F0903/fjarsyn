use crate::ui::message::{Message, NotificationMessage};

pub trait ErrorExt {
    fn to_notify_error(self) -> Message;
}

impl<E: std::fmt::Display> ErrorExt for E {
    fn to_notify_error(self) -> Message {
        Message::Notification(NotificationMessage::NotifyError(self.to_string()))
    }
}

pub trait ResultExt<T> {
    fn map_notify_error(self) -> Result<T, Message>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    fn map_notify_error(self) -> Result<T, Message> {
        self.map_err(|e| e.to_notify_error())
    }
}
