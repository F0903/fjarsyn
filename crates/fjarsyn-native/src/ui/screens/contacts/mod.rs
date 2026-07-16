use std::sync::atomic::{AtomicU64, Ordering};

use fjarsyn_core::{pairing::PairingInvite, peer_session::PeerId};
use iced::{Subscription, Task};

use crate::ui::{
    message::{ContactOperationId, Message},
    screens::Screen,
    shell::{ShellContext, ShellContextMut},
};

pub mod handlers;
mod view;
mod workflow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardRequestId(u64);

impl ClipboardRequestId {
    pub(crate) fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub enum ContactsMessage {
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
    IdentityReplacementRejected { operation_id: ContactOperationId },
    CancelIdentityReplacement,
    RequestDeleteContact(i64),
    ConfirmDeleteContact(i64),
    DeleteContactRejected { operation_id: ContactOperationId, id: i64 },
    CancelDeleteContact,
    AddNewContact,
    NewContactSaveRejected { operation_id: ContactOperationId },
    ToggleAddForm,
}

/// A pairing invite stays unverified until the user explicitly confirms the
/// complete identity fingerprint through an independent trusted channel.
#[derive(Debug, Clone, Default)]
pub(crate) struct PairingDraft {
    pub(crate) invite_text: String,
    pub(crate) invite: Option<PairingInvite>,
    pub(crate) error: Option<String>,
    pub(crate) fingerprint_confirmed: bool,
    pub(crate) clipboard_request: Option<ClipboardRequestId>,
}

impl PairingDraft {
    pub(crate) fn is_ready(&self) -> bool {
        self.invite.is_some() && self.fingerprint_confirmed && self.clipboard_request.is_none()
    }

    pub(crate) fn is_reading_clipboard(&self) -> bool {
        self.clipboard_request.is_some()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IdentityReplacementDraft {
    pub(crate) contact_id: i64,
    pub(crate) expected_peer_id: PeerId,
    pub(crate) pairing: PairingDraft,
    pub(crate) saving: Option<ContactOperationId>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContactDeletionDraft {
    pub(crate) contact_id: i64,
    pub(crate) operation_id: Option<ContactOperationId>,
}

#[derive(Debug, Clone)]
pub struct ContactsScreen {
    pub(crate) new_contact_name: String,
    pub(crate) new_contact_pairing: Box<PairingDraft>,
    pub(crate) saving_new_contact: Option<ContactOperationId>,
    pub(crate) identity_replacement: Option<Box<IdentityReplacementDraft>>,
    pub(crate) contact_deletion: Option<ContactDeletionDraft>,
    pub(crate) show_add_form: bool,
}

impl Default for ContactsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl ContactsScreen {
    pub fn new() -> Self {
        Self {
            new_contact_name: String::new(),
            new_contact_pairing: Box::new(PairingDraft::default()),
            saving_new_contact: None,
            identity_replacement: None,
            contact_deletion: None,
            show_add_form: false,
        }
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: ShellContext<'_>) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut ShellContextMut<'_>, message: Message) -> Task<Message> {
        self.handle_message(ctx, message)
    }

    fn view<'a>(&'a self, ctx: ShellContext<'a>) -> iced::Element<'a, Message> {
        self.render_view(ctx)
    }
}
