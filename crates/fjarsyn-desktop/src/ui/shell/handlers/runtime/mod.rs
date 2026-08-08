//! Runtime lifecycle completions and projection-event handling.

mod event;
mod lifecycle;

pub(in crate::ui::shell) use lifecycle::handle_runtime_msg;
