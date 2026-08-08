//! Contact pairing, trust replacement, and deletion workflow.

mod pairing_draft;
mod screen;
mod view;
mod workflow;

use pairing_draft::PairingDraft;
pub(super) use screen::Screen;
use screen::{DeletionDraft, IdentityReplacementDraft};
