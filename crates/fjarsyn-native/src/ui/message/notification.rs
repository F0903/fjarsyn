#[derive(Debug, Clone)]
pub enum NotificationMessage {
    NotifyError(String),
    NotifyInfo(String),
    NotifySuccess(String),
    DismissNotification(u64),
}
