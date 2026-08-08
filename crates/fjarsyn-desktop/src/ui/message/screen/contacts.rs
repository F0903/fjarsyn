//! Contacts-screen intents and workflow correlation identifiers.

use std::sync::atomic::{AtomicU64, Ordering};

use fjarsyn_engine::identity::PeerId;

/// Process-unique token used to reject stale asynchronous clipboard results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) struct ClipboardRequestId(u64);

impl ClipboardRequestId {
    pub(in crate::ui) fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Process-unique token correlating a contacts-screen workflow with its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) struct OperationId(u64);

impl OperationId {
    pub(in crate::ui) fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    NameChanged(String),
    NewInviteChanged(String),
    PasteNewInvite,
    NewInviteClipboardRead { request_id: ClipboardRequestId, contents: Option<String> },
    NewFingerprintConfirmed(bool),
    StartIdentityReplacement { id: i64, peer_id: PeerId },
    ReplacementInviteChanged(String),
    PasteReplacementInvite,
    ReplacementInviteClipboardRead { request_id: ClipboardRequestId, contents: Option<String> },
    ReplacementFingerprintConfirmed(bool),
    SaveIdentityReplacement,
    IdentityReplacementRejected { operation_id: OperationId },
    CancelIdentityReplacement,
    RequestDeleteContact(i64),
    ConfirmDeleteContact(i64),
    DeleteContactRejected { operation_id: OperationId, id: i64 },
    CancelDeleteContact,
    AddNewContact,
    NewContactSaveRejected { operation_id: OperationId },
    ToggleAddForm,
}
