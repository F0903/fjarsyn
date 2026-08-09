//! Runtime lifecycle completions and engine-adapter output handling.

mod event;
mod lifecycle;

pub(in crate::ui::shell) use lifecycle::handle_runtime_msg;
