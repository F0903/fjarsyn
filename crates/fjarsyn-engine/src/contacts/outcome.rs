use super::Projection;
use crate::{identity::PeerId, peer_session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionWarning {
    pub peer_id: PeerId,
    pub error: peer_session::Error,
}

/// Authoritative contact projection after a committed trust mutation.
///
/// A session-admission warning does not roll back the committed identity. The
/// caller must apply `contacts` and surface the warning; admission remains
/// fail-closed until the peer-session owner can be recovered or restarted.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub projection: Projection,
    pub admission_warning: Option<peer_session::Error>,
}

/// Authoritative contact projection produced by a refresh, together with every
/// peer whose now-definitive trust-mutation barrier could not be released.
/// Those barriers remain retained and a later refresh can retry recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub projection: Projection,
    pub admission_warnings: Vec<AdmissionWarning>,
}
