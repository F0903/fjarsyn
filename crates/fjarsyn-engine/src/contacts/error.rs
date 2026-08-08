use std::sync::Arc;

use super::DirectoryError;
use crate::{identity::PeerId, peer_session};

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("the local peer identity {peer_id} cannot be imported as a contact")]
    SelfIdentity { peer_id: PeerId },
    #[error(
        "verified identity belongs to peer {actual}, but contact {contact_id} belongs to peer {expected}"
    )]
    PeerIdentityMismatch { contact_id: i64, expected: PeerId, actual: PeerId },
    #[error(transparent)]
    Contact(Arc<DirectoryError>),
    #[error(transparent)]
    Session(#[from] peer_session::Error),
    #[error(
        "contact operation failed ({operation}); restoring peer-session admission also failed ({recovery})"
    )]
    Recovery { operation: Arc<DirectoryError>, recovery: peer_session::Error },
    #[error(
        "contact operation outcome could not be proven ({operation}); peer sessions remain suspended: {reconciliation}"
    )]
    OutcomeUnknown { operation: Arc<DirectoryError>, reconciliation: String },
    #[error(
        "peer {peer_id} already has a pending {pending}; reconcile it before starting another trust mutation"
    )]
    PendingReconciliation { peer_id: PeerId, pending: &'static str },
}

impl From<Arc<DirectoryError>> for Error {
    fn from(error: Arc<DirectoryError>) -> Self {
        Self::Contact(error)
    }
}
