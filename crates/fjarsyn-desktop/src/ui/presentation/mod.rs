//! UI-facing projections of presence and peer-session state.

mod context;
mod peer;

pub(in crate::ui) use context::{Context, Inputs};
pub(in crate::ui) use peer::project_peer;
#[cfg(test)]
use peer::{Presence, Session};

/// Formats the complete identity fingerprint as a stable four-group-wide grid.
pub(in crate::ui) fn fingerprint_grid(fingerprint: &str) -> String {
    fingerprint
        .split_whitespace()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|groups| groups.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
