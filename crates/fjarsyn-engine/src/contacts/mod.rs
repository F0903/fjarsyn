//! Contact persistence, trusted-peer resolution, and trust-safe mutations.

mod admission_control;
mod contact;
mod contacts_service;
mod directory;
mod error;
mod outcome;
mod sqlite_store;
mod store;

use admission_control::AdmissionControl;
pub use contact::Contact;
pub use contacts_service::ContactsService;
pub(crate) use directory::Directory;
pub use directory::{DirectoryError, Projection};
pub use error::Error;
pub use outcome::{AdmissionWarning, Outcome, RefreshOutcome};
pub(crate) use sqlite_store::SqliteStore;
pub use store::StoreError;
pub(crate) use store::{ContactRecord, Store};

#[cfg(test)]
mod directory_tests;
#[cfg(test)]
mod tests;
