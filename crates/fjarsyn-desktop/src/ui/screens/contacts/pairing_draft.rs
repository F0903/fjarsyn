use std::str::FromStr;

use fjarsyn_engine::{
    identity::PeerId,
    pairing::{Invite, MAX_INVITE_BYTES},
};

use crate::ui::message::screen::contacts::ClipboardRequestId;

/// A pairing invite stays unverified until the user explicitly confirms the
/// complete identity fingerprint through an independent trusted channel.
#[derive(Debug, Clone, Default)]
pub(super) struct PairingDraft {
    pub(super) invite_text: String,
    pub(super) invite: Option<Invite>,
    pub(super) error: Option<String>,
    pub(super) fingerprint_confirmed: bool,
    pub(super) clipboard_request: Option<ClipboardRequestId>,
}

impl PairingDraft {
    pub(super) fn set_invite(
        &mut self,
        value: String,
        local_peer_id: Option<&PeerId>,
        expected_peer_id: Option<&PeerId>,
    ) {
        self.invite_text.clear();
        self.invite = None;
        self.error = None;
        self.fingerprint_confirmed = false;
        // A direct edit supersedes any clipboard request still in flight. Its
        // eventual completion carries the old request ID and will be ignored.
        self.clipboard_request = None;

        if value.len() > MAX_INVITE_BYTES {
            self.error = Some(format!("Pairing invite exceeds the {MAX_INVITE_BYTES} byte limit."));
            return;
        }
        self.invite_text = value;

        let trimmed = self.invite_text.trim();
        if trimmed.is_empty() {
            self.error = Some("Paste a pairing invite to continue.".into());
            return;
        }

        match Invite::from_str(trimmed) {
            Ok(invite) if local_peer_id == Some(invite.peer_id()) => {
                self.error = Some("You cannot add your own pairing invite.".into());
            }
            Ok(invite) if expected_peer_id.is_some_and(|expected| expected != invite.peer_id()) => {
                self.error = Some(format!(
                    "This invite belongs to {}, but this contact is {}.",
                    invite.peer_id(),
                    expected_peer_id.expect("checked above")
                ));
            }
            Ok(invite) => self.invite = Some(invite),
            Err(error) => self.error = Some(format!("Invalid pairing invite: {error}")),
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.invite.is_some() && self.fingerprint_confirmed && self.clipboard_request.is_none()
    }

    pub(super) fn is_reading_clipboard(&self) -> bool {
        self.clipboard_request.is_some()
    }
}
