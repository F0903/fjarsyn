//! Iced subscriptions for runtime channels, deadlines, and window events.

mod application;
mod deadline;
mod receiver;

pub(in crate::ui) use application::subscription;
pub(in crate::ui) use receiver::Receiver;
