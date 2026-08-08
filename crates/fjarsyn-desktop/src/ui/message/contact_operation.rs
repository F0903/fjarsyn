//! Contact-service operations and their correlated results.

use std::sync::Arc;

use fjarsyn_engine::{contacts, pairing::VerifiedPeerIdentity};

use super::screen::contacts::OperationId;

#[derive(Debug, Clone)]
pub(in crate::ui) enum ContactOperation {
    Save {
        operation_id: OperationId,
        name: String,
        identity: VerifiedPeerIdentity,
    },
    Delete {
        operation_id: OperationId,
        id: i64,
    },
    Saved {
        operation_id: OperationId,
        result: Result<contacts::Outcome, Arc<contacts::Error>>,
    },
    Deleted {
        operation_id: OperationId,
        id: i64,
        result: Result<contacts::Outcome, Arc<contacts::Error>>,
    },
    Updated {
        operation_id: OperationId,
        result: Result<contacts::Outcome, Arc<contacts::Error>>,
    },
    UpdateVerifiedIdentity {
        operation_id: OperationId,
        id: i64,
        identity: VerifiedPeerIdentity,
    },
}
