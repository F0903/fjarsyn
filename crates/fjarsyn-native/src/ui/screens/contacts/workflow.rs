use std::str::FromStr;

use fjarsyn_core::{
    pairing::{MAX_PAIRING_INVITE_BYTES, PairingInvite, VerifiedPeerIdentity},
    peer_session::PeerId,
};

use super::{
    ClipboardRequestId, ContactDeletionDraft, ContactsMessage, ContactsScreen,
    IdentityReplacementDraft, PairingDraft,
};
use crate::ui::message::{ContactOperationId, ContactsServiceMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardTarget {
    NewContact,
    IdentityReplacement,
}

pub(crate) enum ContactsEffect {
    ReadClipboard {
        target: ClipboardTarget,
        request_id: ClipboardRequestId,
    },
    SaveContact {
        operation_id: ContactOperationId,
        name: String,
        identity: VerifiedPeerIdentity,
    },
    UpdateVerifiedIdentity {
        operation_id: ContactOperationId,
        id: i64,
        identity: VerifiedPeerIdentity,
    },
    DeleteContact {
        operation_id: ContactOperationId,
        id: i64,
    },
}

pub(crate) fn execute_contacts_message(
    screen: &mut ContactsScreen,
    message: ContactsMessage,
    local_peer_id: Option<&PeerId>,
) -> Vec<ContactsEffect> {
    match message {
        ContactsMessage::NameChanged(value) => {
            if screen.saving_new_contact.is_none() {
                screen.new_contact_name = value;
            }
            Vec::new()
        }
        ContactsMessage::NewInviteChanged(value) => {
            if screen.saving_new_contact.is_none() {
                set_pairing_invite(&mut screen.new_contact_pairing, value, local_peer_id, None);
            }
            Vec::new()
        }
        ContactsMessage::PasteNewInvite => {
            begin_clipboard_read(screen, ClipboardTarget::NewContact).into_iter().collect()
        }
        ContactsMessage::NewInviteClipboardRead { request_id, contents } => {
            finish_new_contact_clipboard_read(screen, request_id, contents, local_peer_id);
            Vec::new()
        }
        ContactsMessage::NewFingerprintConfirmed(confirmed) => {
            if screen.saving_new_contact.is_none() {
                screen.new_contact_pairing.fingerprint_confirmed =
                    confirmed && screen.new_contact_pairing.invite.is_some();
            }
            Vec::new()
        }
        ContactsMessage::StartIdentityReplacement { id, peer_id } => {
            if screen.identity_replacement.is_none()
                && !screen
                    .contact_deletion
                    .as_ref()
                    .is_some_and(|deletion| deletion.contact_id == id)
            {
                screen.identity_replacement = Some(Box::new(IdentityReplacementDraft {
                    contact_id: id,
                    expected_peer_id: peer_id,
                    pairing: PairingDraft::default(),
                    saving: None,
                }));
            }
            Vec::new()
        }
        ContactsMessage::ReplacementInviteChanged(value) => {
            if let Some(replacement) = screen
                .identity_replacement
                .as_mut()
                .filter(|replacement| replacement.saving.is_none())
            {
                set_pairing_invite(
                    &mut replacement.pairing,
                    value,
                    local_peer_id,
                    Some(&replacement.expected_peer_id),
                );
            }
            Vec::new()
        }
        ContactsMessage::PasteReplacementInvite => {
            begin_clipboard_read(screen, ClipboardTarget::IdentityReplacement).into_iter().collect()
        }
        ContactsMessage::ReplacementInviteClipboardRead { request_id, contents } => {
            finish_replacement_clipboard_read(screen, request_id, contents, local_peer_id);
            Vec::new()
        }
        ContactsMessage::ReplacementFingerprintConfirmed(confirmed) => {
            if let Some(replacement) = screen
                .identity_replacement
                .as_mut()
                .filter(|replacement| replacement.saving.is_none())
            {
                replacement.pairing.fingerprint_confirmed =
                    confirmed && replacement.pairing.invite.is_some();
            }
            Vec::new()
        }
        ContactsMessage::CancelIdentityReplacement => {
            if !screen.identity_replacement.as_ref().is_some_and(|draft| draft.saving.is_some()) {
                screen.identity_replacement = None;
            }
            Vec::new()
        }
        ContactsMessage::RequestDeleteContact(id) => {
            let identity_open = screen
                .identity_replacement
                .as_ref()
                .is_some_and(|replacement| replacement.contact_id == id);
            if !identity_open && screen.contact_deletion.is_none() {
                screen.contact_deletion =
                    Some(ContactDeletionDraft { contact_id: id, operation_id: None });
            }
            Vec::new()
        }
        ContactsMessage::ConfirmDeleteContact(id) => {
            let Some(deletion) = screen
                .contact_deletion
                .as_mut()
                .filter(|deletion| deletion.contact_id == id && deletion.operation_id.is_none())
            else {
                return Vec::new();
            };
            let operation_id = ContactOperationId::next();
            deletion.operation_id = Some(operation_id);
            vec![ContactsEffect::DeleteContact { operation_id, id }]
        }
        ContactsMessage::DeleteContactRejected { operation_id, id } => {
            finish_contact_delete(screen, operation_id, id, false);
            Vec::new()
        }
        ContactsMessage::CancelDeleteContact => {
            if !screen
                .contact_deletion
                .as_ref()
                .is_some_and(|deletion| deletion.operation_id.is_some())
            {
                screen.contact_deletion = None;
            }
            Vec::new()
        }
        ContactsMessage::SaveIdentityReplacement => {
            build_update_verified_identity_effect(screen).into_iter().collect()
        }
        ContactsMessage::IdentityReplacementRejected { operation_id } => {
            finish_identity_replacement(screen, operation_id, false);
            Vec::new()
        }
        ContactsMessage::ToggleAddForm => {
            if screen.saving_new_contact.is_none() {
                if screen.show_add_form {
                    screen.new_contact_pairing.clipboard_request = None;
                }
                screen.show_add_form = !screen.show_add_form;
            }
            Vec::new()
        }
        ContactsMessage::AddNewContact => {
            build_save_contact_effect(screen, local_peer_id).into_iter().collect()
        }
        ContactsMessage::NewContactSaveRejected { operation_id } => {
            finish_contact_save(screen, operation_id, false);
            Vec::new()
        }
    }
}

fn begin_clipboard_read(
    screen: &mut ContactsScreen,
    target: ClipboardTarget,
) -> Option<ContactsEffect> {
    let already_reading = match target {
        ClipboardTarget::NewContact => {
            screen.saving_new_contact.is_some() || screen.new_contact_pairing.is_reading_clipboard()
        }
        ClipboardTarget::IdentityReplacement => {
            screen.identity_replacement.as_ref().is_none_or(|replacement| {
                replacement.saving.is_some() || replacement.pairing.is_reading_clipboard()
            })
        }
    };
    if already_reading {
        return None;
    }

    let request_id = ClipboardRequestId::next();
    match target {
        ClipboardTarget::NewContact => {
            screen.new_contact_pairing.clipboard_request = Some(request_id);
            screen.new_contact_pairing.fingerprint_confirmed = false;
        }
        ClipboardTarget::IdentityReplacement => {
            let pairing = &mut screen.identity_replacement.as_mut()?.pairing;
            pairing.clipboard_request = Some(request_id);
            pairing.fingerprint_confirmed = false;
        }
    }
    Some(ContactsEffect::ReadClipboard { target, request_id })
}

fn finish_new_contact_clipboard_read(
    screen: &mut ContactsScreen,
    request_id: ClipboardRequestId,
    contents: Option<String>,
    local_peer_id: Option<&PeerId>,
) {
    if screen.new_contact_pairing.clipboard_request != Some(request_id) {
        return;
    }
    if screen.saving_new_contact.is_some() {
        screen.new_contact_pairing.clipboard_request = None;
        return;
    }
    screen.new_contact_pairing.clipboard_request = None;
    set_pairing_invite(
        &mut screen.new_contact_pairing,
        contents.unwrap_or_default(),
        local_peer_id,
        None,
    );
}

fn finish_replacement_clipboard_read(
    screen: &mut ContactsScreen,
    request_id: ClipboardRequestId,
    contents: Option<String>,
    local_peer_id: Option<&PeerId>,
) {
    let Some(replacement) = screen.identity_replacement.as_mut() else {
        return;
    };
    if replacement.pairing.clipboard_request != Some(request_id) {
        return;
    }
    if replacement.saving.is_some() {
        replacement.pairing.clipboard_request = None;
        return;
    }
    replacement.pairing.clipboard_request = None;
    set_pairing_invite(
        &mut replacement.pairing,
        contents.unwrap_or_default(),
        local_peer_id,
        Some(&replacement.expected_peer_id),
    );
}

fn set_pairing_invite(
    draft: &mut PairingDraft,
    value: String,
    local_peer_id: Option<&PeerId>,
    expected_peer_id: Option<&PeerId>,
) {
    draft.invite_text.clear();
    draft.invite = None;
    draft.error = None;
    draft.fingerprint_confirmed = false;
    // A direct edit supersedes any clipboard request still in flight. Its
    // eventual completion carries the old request ID and will be ignored.
    draft.clipboard_request = None;

    if value.len() > MAX_PAIRING_INVITE_BYTES {
        draft.error =
            Some(format!("Pairing invite exceeds the {MAX_PAIRING_INVITE_BYTES} byte limit."));
        return;
    }
    draft.invite_text = value;

    let trimmed = draft.invite_text.trim();
    if trimmed.is_empty() {
        draft.error = Some("Paste a pairing invite to continue.".into());
        return;
    }

    match PairingInvite::from_str(trimmed) {
        Ok(invite) if local_peer_id == Some(invite.peer_id()) => {
            draft.error = Some("You cannot add your own pairing invite.".into());
        }
        Ok(invite) if expected_peer_id.is_some_and(|expected| expected != invite.peer_id()) => {
            draft.error = Some(format!(
                "This invite belongs to {}, but this contact is {}.",
                invite.peer_id(),
                expected_peer_id.expect("checked above")
            ));
        }
        Ok(invite) => draft.invite = Some(invite),
        Err(error) => draft.error = Some(format!("Invalid pairing invite: {error}")),
    }
}

fn build_save_contact_effect(
    screen: &mut ContactsScreen,
    local_peer_id: Option<&PeerId>,
) -> Option<ContactsEffect> {
    if screen.saving_new_contact.is_some() || local_peer_id.is_none() {
        return None;
    }
    let name = screen.new_contact_name.trim().to_owned();
    if name.is_empty() || !screen.new_contact_pairing.is_ready() {
        return None;
    }
    let invite = screen.new_contact_pairing.invite.clone()?;
    if local_peer_id == Some(invite.peer_id()) {
        screen.new_contact_pairing.fingerprint_confirmed = false;
        screen.new_contact_pairing.error = Some("You cannot add your own pairing invite.".into());
        return None;
    }

    let operation_id = ContactOperationId::next();
    screen.saving_new_contact = Some(operation_id);
    Some(ContactsEffect::SaveContact { operation_id, name, identity: invite.confirm() })
}

fn build_update_verified_identity_effect(screen: &mut ContactsScreen) -> Option<ContactsEffect> {
    let replacement = screen.identity_replacement.as_mut()?;
    if replacement.saving.is_some() || !replacement.pairing.is_ready() {
        return None;
    }
    let invite = replacement.pairing.invite.clone()?;
    if invite.peer_id() != &replacement.expected_peer_id {
        replacement.pairing.fingerprint_confirmed = false;
        replacement.pairing.error = Some(format!(
            "This invite belongs to {}, but this contact is {}.",
            invite.peer_id(),
            replacement.expected_peer_id
        ));
        return None;
    }

    let operation_id = ContactOperationId::next();
    replacement.saving = Some(operation_id);
    Some(ContactsEffect::UpdateVerifiedIdentity {
        operation_id,
        id: replacement.contact_id,
        identity: invite.confirm(),
    })
}

pub(crate) fn finish_contact_save(
    screen: &mut ContactsScreen,
    operation_id: ContactOperationId,
    succeeded: bool,
) {
    if screen.saving_new_contact != Some(operation_id) {
        return;
    }
    if succeeded {
        screen.new_contact_name.clear();
        *screen.new_contact_pairing = PairingDraft::default();
        screen.saving_new_contact = None;
        screen.show_add_form = false;
    } else {
        screen.saving_new_contact = None;
    }
}

pub(crate) fn finish_identity_replacement(
    screen: &mut ContactsScreen,
    operation_id: ContactOperationId,
    succeeded: bool,
) {
    if !screen
        .identity_replacement
        .as_ref()
        .is_some_and(|replacement| replacement.saving == Some(operation_id))
    {
        return;
    }
    if succeeded {
        screen.identity_replacement = None;
    } else if let Some(replacement) = screen.identity_replacement.as_mut() {
        replacement.saving = None;
    }
}

pub(crate) fn finish_contact_delete(
    screen: &mut ContactsScreen,
    operation_id: ContactOperationId,
    id: i64,
    succeeded: bool,
) {
    let Some(deletion) = screen.contact_deletion.as_mut().filter(|deletion| {
        deletion.contact_id == id && deletion.operation_id == Some(operation_id)
    }) else {
        return;
    };
    if succeeded {
        screen.contact_deletion = None;
    } else {
        deletion.operation_id = None;
    }
}

pub(crate) fn into_service_message(effect: ContactsEffect) -> Option<ContactsServiceMessage> {
    match effect {
        ContactsEffect::SaveContact { operation_id, name, identity } => {
            Some(ContactsServiceMessage::SaveContact { operation_id, name, identity })
        }
        ContactsEffect::UpdateVerifiedIdentity { operation_id, id, identity } => {
            Some(ContactsServiceMessage::UpdateContactVerifiedIdentity {
                operation_id,
                id,
                identity,
            })
        }
        ContactsEffect::DeleteContact { operation_id, id } => {
            Some(ContactsServiceMessage::DeleteContact { operation_id, id })
        }
        ContactsEffect::ReadClipboard { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_core::{identity::LocalPeerIdentity, pairing::PairingInvite};

    use super::*;

    fn local() -> (PeerId, LocalPeerIdentity) {
        (PeerId::new("local").unwrap(), LocalPeerIdentity::generate())
    }

    fn invite(peer_id: &str) -> PairingInvite {
        PairingInvite::from_local(PeerId::new(peer_id).unwrap(), &LocalPeerIdentity::generate())
    }

    fn visible_screen() -> ContactsScreen {
        ContactsScreen { show_add_form: true, ..ContactsScreen::new() }
    }

    fn paste_new(screen: &mut ContactsScreen, local_peer_id: &PeerId, contents: Option<String>) {
        let effects =
            execute_contacts_message(screen, ContactsMessage::PasteNewInvite, Some(local_peer_id));
        let [ContactsEffect::ReadClipboard { request_id, .. }] = effects.as_slice() else {
            panic!("clipboard read was not requested")
        };
        execute_contacts_message(
            screen,
            ContactsMessage::NewInviteClipboardRead { request_id: *request_id, contents },
            Some(local_peer_id),
        );
    }

    #[test]
    fn empty_and_invalid_invites_stay_unverified() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();

        paste_new(&mut screen, &local_peer_id, Some("  ".into()));
        assert!(screen.new_contact_pairing.invite.is_none());
        assert!(screen.new_contact_pairing.error.is_some());

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteChanged("not-an-invite".into()),
            Some(&local_peer_id),
        );
        assert!(screen.new_contact_pairing.invite.is_none());
        assert!(screen.new_contact_pairing.error.as_deref().unwrap().contains("Invalid"));
    }

    #[test]
    fn valid_second_paste_resets_fingerprint_confirmation() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        paste_new(&mut screen, &local_peer_id, Some(invite("first").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert!(screen.new_contact_pairing.fingerprint_confirmed);

        paste_new(&mut screen, &local_peer_id, Some(invite("second").to_string()));

        assert_eq!(
            screen.new_contact_pairing.invite.as_ref().unwrap().peer_id().as_str(),
            "second"
        );
        assert!(!screen.new_contact_pairing.fingerprint_confirmed);
    }

    #[test]
    fn overlapping_clipboard_reads_are_prevented_and_stale_results_are_ignored() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        let first = execute_contacts_message(
            &mut screen,
            ContactsMessage::PasteNewInvite,
            Some(&local_peer_id),
        );
        assert_eq!(first.len(), 1);
        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::PasteNewInvite,
                Some(&local_peer_id),
            )
            .is_empty()
        );
        let ContactsEffect::ReadClipboard { request_id, .. } = first[0] else { unreachable!() };

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteChanged(invite("typed").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteClipboardRead {
                request_id,
                contents: Some(invite("stale").to_string()),
            },
            Some(&local_peer_id),
        );
        assert_eq!(screen.new_contact_pairing.invite.as_ref().unwrap().peer_id().as_str(), "typed");
    }

    #[test]
    fn paste_immediately_revokes_confirmation_and_blocks_save_until_completion() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert!(screen.new_contact_pairing.is_ready());

        let effects = execute_contacts_message(
            &mut screen,
            ContactsMessage::PasteNewInvite,
            Some(&local_peer_id),
        );

        assert_eq!(effects.len(), 1);
        assert!(!screen.new_contact_pairing.fingerprint_confirmed);
        assert!(!screen.new_contact_pairing.is_ready());
        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .is_empty()
        );
    }

    #[test]
    fn clipboard_request_ids_do_not_collide_across_screen_instances() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        let old_effects = execute_contacts_message(
            &mut old_screen,
            ContactsMessage::PasteNewInvite,
            Some(&local_peer_id),
        );
        let [ContactsEffect::ReadClipboard { request_id: old_request, .. }] =
            old_effects.as_slice()
        else {
            panic!("old clipboard read was not requested")
        };

        let mut new_screen = visible_screen();
        let new_effects = execute_contacts_message(
            &mut new_screen,
            ContactsMessage::PasteNewInvite,
            Some(&local_peer_id),
        );
        let [ContactsEffect::ReadClipboard { request_id: new_request, .. }] =
            new_effects.as_slice()
        else {
            panic!("new clipboard read was not requested")
        };
        assert_ne!(old_request, new_request);

        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::NewInviteClipboardRead {
                request_id: *old_request,
                contents: Some(invite("stale").to_string()),
            },
            Some(&local_peer_id),
        );

        assert_eq!(new_screen.new_contact_pairing.clipboard_request, Some(*new_request));
        assert!(new_screen.new_contact_pairing.invite.is_none());
    }

    #[test]
    fn clipboard_completion_cannot_mutate_an_admitted_save() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, ContactsMessage::AddNewContact, Some(&local_peer_id));
        let request_id = ClipboardRequestId::next();
        screen.new_contact_pairing.clipboard_request = Some(request_id);

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteClipboardRead {
                request_id,
                contents: Some(invite("mallory").to_string()),
            },
            Some(&local_peer_id),
        );

        assert_eq!(screen.new_contact_pairing.invite.as_ref().unwrap().peer_id().as_str(), "alice");
    }

    #[test]
    fn closing_the_form_invalidates_a_clipboard_read_and_reopen_can_retry() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        let effects = execute_contacts_message(
            &mut screen,
            ContactsMessage::PasteNewInvite,
            Some(&local_peer_id),
        );
        let [ContactsEffect::ReadClipboard { request_id, .. }] = effects.as_slice() else {
            panic!("clipboard read was not requested")
        };

        execute_contacts_message(&mut screen, ContactsMessage::ToggleAddForm, Some(&local_peer_id));
        execute_contacts_message(&mut screen, ContactsMessage::ToggleAddForm, Some(&local_peer_id));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteClipboardRead {
                request_id: *request_id,
                contents: Some(invite("stale").to_string()),
            },
            Some(&local_peer_id),
        );

        assert!(screen.show_add_form);
        assert!(screen.new_contact_pairing.invite.is_none());
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::PasteNewInvite,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
    }

    #[test]
    fn oversized_invite_is_rejected_without_being_retained_for_rendering() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewInviteChanged("x".repeat(MAX_PAIRING_INVITE_BYTES + 1)),
            Some(&local_peer_id),
        );

        assert!(screen.new_contact_pairing.invite_text.is_empty());
        assert!(screen.new_contact_pairing.invite.is_none());
        assert!(screen.new_contact_pairing.error.as_deref().unwrap().contains("512"));
    }

    #[test]
    fn unconfirmed_invite_cannot_be_saved_and_failure_preserves_the_draft() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));

        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .is_empty()
        );

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
        let operation_id = screen.saving_new_contact.expect("save is pending");
        finish_contact_save(&mut screen, operation_id, false);
        assert_eq!(screen.new_contact_name, "Alice");
        assert_eq!(screen.new_contact_pairing.invite.as_ref().unwrap().peer_id().as_str(), "alice");
        assert!(screen.new_contact_pairing.fingerprint_confirmed);
    }

    #[test]
    fn self_invite_is_rejected_before_persistence() {
        let (local_peer_id, identity) = local();
        let local_invite = PairingInvite::from_local(local_peer_id.clone(), &identity);
        let mut screen = visible_screen();

        paste_new(&mut screen, &local_peer_id, Some(local_invite.to_string()));

        assert!(screen.new_contact_pairing.invite.is_none());
        assert!(screen.new_contact_pairing.error.as_deref().unwrap().contains("own"));
    }

    #[test]
    fn identity_replacement_requires_the_same_peer_and_fresh_confirmation() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        execute_contacts_message(
            &mut screen,
            ContactsMessage::StartIdentityReplacement {
                id: 7,
                peer_id: PeerId::new("alice").unwrap(),
            },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::ReplacementInviteChanged(invite("mallory").to_string()),
            Some(&local_peer_id),
        );
        let replacement = screen.identity_replacement.as_ref().unwrap();
        assert!(replacement.pairing.invite.is_none());
        assert!(replacement.pairing.error.as_deref().unwrap().contains("alice"));

        execute_contacts_message(
            &mut screen,
            ContactsMessage::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        assert!(!screen.identity_replacement.as_ref().unwrap().pairing.fingerprint_confirmed);
        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::SaveIdentityReplacement,
                Some(&local_peer_id),
            )
            .is_empty()
        );
    }

    #[test]
    fn a_second_replacement_request_never_discards_the_open_draft() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        execute_contacts_message(
            &mut screen,
            ContactsMessage::StartIdentityReplacement {
                id: 7,
                peer_id: PeerId::new("alice").unwrap(),
            },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );

        execute_contacts_message(
            &mut screen,
            ContactsMessage::StartIdentityReplacement {
                id: 8,
                peer_id: PeerId::new("bob").unwrap(),
            },
            Some(&local_peer_id),
        );

        let replacement = screen.identity_replacement.as_ref().unwrap();
        assert_eq!(replacement.contact_id, 7);
        assert_eq!(replacement.expected_peer_id.as_str(), "alice");
        assert_eq!(replacement.pairing.invite.as_ref().unwrap().peer_id().as_str(), "alice");
    }

    #[test]
    fn rejected_identity_replacement_preserves_confirmation_and_unlocks_retry() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        execute_contacts_message(
            &mut screen,
            ContactsMessage::StartIdentityReplacement {
                id: 7,
                peer_id: PeerId::new("alice").unwrap(),
            },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::SaveIdentityReplacement,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
        let operation_id =
            screen.identity_replacement.as_ref().unwrap().saving.expect("replace is pending");

        execute_contacts_message(
            &mut screen,
            ContactsMessage::IdentityReplacementRejected { operation_id },
            Some(&local_peer_id),
        );

        let replacement = screen.identity_replacement.as_ref().unwrap();
        assert!(replacement.saving.is_none());
        assert!(replacement.pairing.is_ready());
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::SaveIdentityReplacement,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
        let operation_id =
            screen.identity_replacement.as_ref().unwrap().saving.expect("retry is pending");
        finish_identity_replacement(&mut screen, operation_id, true);
        assert!(screen.identity_replacement.is_none());
    }

    #[test]
    fn stale_identity_update_completion_cannot_clear_a_newer_replacement() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::StartIdentityReplacement {
                id: 7,
                peer_id: PeerId::new("alice").unwrap(),
            },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::SaveIdentityReplacement,
            Some(&local_peer_id),
        );
        let old_operation = old_screen.identity_replacement.as_ref().unwrap().saving.unwrap();

        let mut new_screen = visible_screen();
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::StartIdentityReplacement {
                id: 7,
                peer_id: PeerId::new("alice").unwrap(),
            },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::SaveIdentityReplacement,
            Some(&local_peer_id),
        );
        let new_operation = new_screen.identity_replacement.as_ref().unwrap().saving.unwrap();
        assert_ne!(old_operation, new_operation);

        finish_identity_replacement(&mut new_screen, old_operation, true);

        assert_eq!(new_screen.identity_replacement.as_ref().unwrap().saving, Some(new_operation));
        finish_identity_replacement(&mut new_screen, new_operation, true);
        assert!(new_screen.identity_replacement.is_none());
    }

    #[test]
    fn delete_requires_an_explicit_matching_confirmation() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();

        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::RequestDeleteContact(7),
                Some(&local_peer_id),
            )
            .is_empty()
        );
        assert_eq!(screen.contact_deletion.as_ref().map(|draft| draft.contact_id), Some(7));
        assert!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::ConfirmDeleteContact(8),
                Some(&local_peer_id),
            )
            .is_empty()
        );
        let effects = execute_contacts_message(
            &mut screen,
            ContactsMessage::ConfirmDeleteContact(7),
            Some(&local_peer_id),
        );

        assert!(matches!(effects.as_slice(), [ContactsEffect::DeleteContact { id: 7, .. }]));
        assert!(screen.contact_deletion.as_ref().unwrap().operation_id.is_some());
    }

    #[test]
    fn stale_delete_completion_cannot_clear_a_newer_confirmation() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::RequestDeleteContact(7),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::ConfirmDeleteContact(7),
            Some(&local_peer_id),
        );
        let old_operation = old_screen.contact_deletion.as_ref().unwrap().operation_id.unwrap();

        let mut new_screen = visible_screen();
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::RequestDeleteContact(8),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::ConfirmDeleteContact(8),
            Some(&local_peer_id),
        );
        let new_operation = new_screen.contact_deletion.as_ref().unwrap().operation_id.unwrap();

        finish_contact_delete(&mut new_screen, old_operation, 7, true);

        assert_eq!(new_screen.contact_deletion.as_ref().unwrap().contact_id, 8);
        assert_eq!(new_screen.contact_deletion.as_ref().unwrap().operation_id, Some(new_operation));
        finish_contact_delete(&mut new_screen, new_operation, 8, true);
        assert!(new_screen.contact_deletion.is_none());
    }

    #[test]
    fn successful_results_are_the_only_automatic_draft_clear() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .len(),
            1
        );

        let operation_id = screen.saving_new_contact.expect("save is pending");
        finish_contact_save(&mut screen, operation_id, true);

        assert!(screen.new_contact_name.is_empty());
        assert!(screen.new_contact_pairing.invite.is_none());
        assert!(!screen.show_add_form);
    }

    #[test]
    fn stale_save_completion_cannot_mutate_a_newer_screen_draft() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        old_screen.new_contact_name = "Old".into();
        paste_new(&mut old_screen, &local_peer_id, Some(invite("old").to_string()));
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            ContactsMessage::AddNewContact,
            Some(&local_peer_id),
        );
        let old_operation = old_screen.saving_new_contact.unwrap();

        let mut new_screen = visible_screen();
        new_screen.new_contact_name = "New".into();
        paste_new(&mut new_screen, &local_peer_id, Some(invite("new").to_string()));
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            ContactsMessage::AddNewContact,
            Some(&local_peer_id),
        );
        let new_operation = new_screen.saving_new_contact.unwrap();
        assert_ne!(old_operation, new_operation);

        finish_contact_save(&mut new_screen, old_operation, true);

        assert_eq!(new_screen.new_contact_name, "New");
        assert_eq!(new_screen.saving_new_contact, Some(new_operation));
        finish_contact_save(&mut new_screen, new_operation, true);
        assert!(new_screen.new_contact_name.is_empty());
    }

    #[test]
    fn admitted_save_freezes_its_request_snapshot() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, ContactsMessage::AddNewContact, Some(&local_peer_id));

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NameChanged("Changed".into()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(false),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, ContactsMessage::ToggleAddForm, Some(&local_peer_id));

        assert_eq!(screen.new_contact_name, "Alice");
        assert!(screen.new_contact_pairing.fingerprint_confirmed);
        assert!(screen.show_add_form);
    }

    #[test]
    fn rejected_completion_preserves_the_draft_and_unlocks_retry() {
        let (local_peer_id, _) = local();
        let mut screen = visible_screen();
        screen.new_contact_name = "Alice".into();
        paste_new(&mut screen, &local_peer_id, Some(invite("alice").to_string()));
        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
        let operation_id = screen.saving_new_contact.expect("save is pending");

        execute_contacts_message(
            &mut screen,
            ContactsMessage::NewContactSaveRejected { operation_id },
            Some(&local_peer_id),
        );

        assert!(screen.saving_new_contact.is_none());
        assert_eq!(screen.new_contact_name, "Alice");
        assert!(screen.new_contact_pairing.is_ready());
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                ContactsMessage::AddNewContact,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
    }
}
