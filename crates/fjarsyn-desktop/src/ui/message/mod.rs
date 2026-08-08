//! Top-level UI messages and domain-specific message families.

pub(in crate::ui) mod peer;
pub(in crate::ui) mod screen;
pub(in crate::ui) mod window;

#[path = "message.rs"]
mod aggregate;
mod contact_operation;
mod route;

pub(in crate::ui) use aggregate::{Config, Lifecycle, Message, Navigation, Notification, Runtime};
pub(in crate::ui) use contact_operation::ContactOperation;
pub(in crate::ui) use route::Route;
pub(in crate::ui) use screen::Screen;
