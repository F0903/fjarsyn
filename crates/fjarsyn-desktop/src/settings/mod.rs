//! Desktop-owned settings and their persistence boundary.

#[path = "settings.rs"]
mod model;
mod store;

pub(crate) use model::{PowerPreference, Settings};
pub(crate) use store::Store;
