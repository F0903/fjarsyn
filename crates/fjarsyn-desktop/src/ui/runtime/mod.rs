//! Desktop ownership around the headless engine and UI projections.

mod application;
mod event;
mod projection;

pub(in crate::ui) use application::{Application, Slot};
pub(in crate::ui) use event::Event;
