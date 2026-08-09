//! Persistent messaging over authenticated peer sessions.
//!
//! The module owns the messaging lifecycle, transport adapter, persistence
//! projection, and cohesive public conversation model.

mod actor;
mod conversation;
mod conversations;
mod error;
mod event;
mod messaging_service;
mod service_handle;
mod sqlite_store;
mod store;
mod transport;

pub(crate) const SESSION_EVENT_CAPACITY: usize = 256;
pub(super) const COMMAND_CAPACITY: usize = 32;
const EVENT_CAPACITY: usize = 256;

#[cfg(test)]
mod tests;

pub use conversation::{
    ConversationMap, ConversationMessage, ConversationSummary, MessageDirection,
    MessageRecordError, MessageStatus,
};
pub use conversations::Conversations;
pub use error::Error;
pub use event::Event;
pub(crate) use messaging_service::{Config, Limits, MessagingService};
pub use service_handle::ServiceHandle;
pub(crate) use sqlite_store::SqliteStore;
pub use store::StoreError;
pub(crate) use store::{MessageRecord, Store};
