use fjarsyn_engine::{identity::PeerId, pairing::VerifiedPeerIdentity};

use super::{DeletionDraft, IdentityReplacementDraft, PairingDraft, Screen};
use crate::ui::message::{
    self,
    screen::contacts::{ClipboardRequestId, Message, OperationId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClipboardTarget {
    NewContact,
    IdentityReplacement,
}

pub(super) enum Effect {
    ReadClipboard { target: ClipboardTarget, request_id: ClipboardRequestId },
    SaveContact { operation_id: OperationId, name: String, identity: VerifiedPeerIdentity },
    UpdateVerifiedIdentity { operation_id: OperationId, id: i64, identity: VerifiedPeerIdentity },
    DeleteContact { operation_id: OperationId, id: i64 },
}

pub(super) fn execute_contacts_message(
    screen: &mut Screen,
    message: Message,
    local_peer_id: Option<&PeerId>,
) -> Vec<Effect> {
    match message {
        Message::NameChanged(value) => {
            if screen.saving_new_contact.is_none() {
                screen.new_contact_name = value;
            }
            Vec::new()
        }
        Message::NewInviteChanged(value) => {
            if screen.saving_new_contact.is_none() {
                screen.new_contact_pairing.set_invite(value, local_peer_id, None);
            }
            Vec::new()
        }
        Message::PasteNewInvite => {
            begin_clipboard_read(screen, ClipboardTarget::NewContact).into_iter().collect()
        }
        Message::NewInviteClipboardRead { request_id, contents } => {
            finish_new_contact_clipboard_read(screen, request_id, contents, local_peer_id);
            Vec::new()
        }
        Message::NewFingerprintConfirmed(confirmed) => {
            if screen.saving_new_contact.is_none() {
                screen.new_contact_pairing.fingerprint_confirmed =
                    confirmed && screen.new_contact_pairing.invite.is_some();
            }
            Vec::new()
        }
        Message::StartIdentityReplacement { id, peer_id } => {
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
        Message::ReplacementInviteChanged(value) => {
            if let Some(replacement) = screen
                .identity_replacement
                .as_mut()
                .filter(|replacement| replacement.saving.is_none())
            {
                replacement.pairing.set_invite(
                    value,
                    local_peer_id,
                    Some(&replacement.expected_peer_id),
                );
            }
            Vec::new()
        }
        Message::PasteReplacementInvite => {
            begin_clipboard_read(screen, ClipboardTarget::IdentityReplacement).into_iter().collect()
        }
        Message::ReplacementInviteClipboardRead { request_id, contents } => {
            finish_replacement_clipboard_read(screen, request_id, contents, local_peer_id);
            Vec::new()
        }
        Message::ReplacementFingerprintConfirmed(confirmed) => {
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
        Message::CancelIdentityReplacement => {
            if !screen.identity_replacement.as_ref().is_some_and(|draft| draft.saving.is_some()) {
                screen.identity_replacement = None;
            }
            Vec::new()
        }
        Message::RequestDeleteContact(id) => {
            let identity_open = screen
                .identity_replacement
                .as_ref()
                .is_some_and(|replacement| replacement.contact_id == id);
            if !identity_open && screen.contact_deletion.is_none() {
                screen.contact_deletion =
                    Some(DeletionDraft { contact_id: id, operation_id: None });
            }
            Vec::new()
        }
        Message::ConfirmDeleteContact(id) => {
            let Some(deletion) = screen
                .contact_deletion
                .as_mut()
                .filter(|deletion| deletion.contact_id == id && deletion.operation_id.is_none())
            else {
                return Vec::new();
            };
            let operation_id = OperationId::next();
            deletion.operation_id = Some(operation_id);
            vec![Effect::DeleteContact { operation_id, id }]
        }
        Message::DeleteContactRejected { operation_id, id } => {
            finish_contact_delete(screen, operation_id, id, false);
            Vec::new()
        }
        Message::CancelDeleteContact => {
            if !screen
                .contact_deletion
                .as_ref()
                .is_some_and(|deletion| deletion.operation_id.is_some())
            {
                screen.contact_deletion = None;
            }
            Vec::new()
        }
        Message::SaveIdentityReplacement => {
            build_update_verified_identity_effect(screen).into_iter().collect()
        }
        Message::IdentityReplacementRejected { operation_id } => {
            finish_identity_replacement(screen, operation_id, false);
            Vec::new()
        }
        Message::ToggleAddForm => {
            if screen.saving_new_contact.is_none() {
                if screen.show_add_form {
                    screen.new_contact_pairing.clipboard_request = None;
                }
                screen.show_add_form = !screen.show_add_form;
            }
            Vec::new()
        }
        Message::AddNewContact => {
            build_save_contact_effect(screen, local_peer_id).into_iter().collect()
        }
        Message::NewContactSaveRejected { operation_id } => {
            finish_contact_save(screen, operation_id, false);
            Vec::new()
        }
    }
}

fn begin_clipboard_read(screen: &mut Screen, target: ClipboardTarget) -> Option<Effect> {
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
    Some(Effect::ReadClipboard { target, request_id })
}

fn finish_new_contact_clipboard_read(
    screen: &mut Screen,
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
    screen.new_contact_pairing.set_invite(contents.unwrap_or_default(), local_peer_id, None);
}

fn finish_replacement_clipboard_read(
    screen: &mut Screen,
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
    replacement.pairing.set_invite(
        contents.unwrap_or_default(),
        local_peer_id,
        Some(&replacement.expected_peer_id),
    );
}

fn build_save_contact_effect(
    screen: &mut Screen,
    local_peer_id: Option<&PeerId>,
) -> Option<Effect> {
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

    let operation_id = OperationId::next();
    screen.saving_new_contact = Some(operation_id);
    Some(Effect::SaveContact { operation_id, name, identity: invite.confirm() })
}

fn build_update_verified_identity_effect(screen: &mut Screen) -> Option<Effect> {
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

    let operation_id = OperationId::next();
    replacement.saving = Some(operation_id);
    Some(Effect::UpdateVerifiedIdentity {
        operation_id,
        id: replacement.contact_id,
        identity: invite.confirm(),
    })
}

pub(super) fn finish_contact_save(screen: &mut Screen, operation_id: OperationId, succeeded: bool) {
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

pub(super) fn finish_identity_replacement(
    screen: &mut Screen,
    operation_id: OperationId,
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

pub(super) fn finish_contact_delete(
    screen: &mut Screen,
    operation_id: OperationId,
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

pub(super) fn into_contact_operation(effect: Effect) -> Option<message::ContactOperation> {
    match effect {
        Effect::SaveContact { operation_id, name, identity } => {
            Some(message::ContactOperation::Save { operation_id, name, identity })
        }
        Effect::UpdateVerifiedIdentity { operation_id, id, identity } => {
            Some(message::ContactOperation::UpdateVerifiedIdentity { operation_id, id, identity })
        }
        Effect::DeleteContact { operation_id, id } => {
            Some(message::ContactOperation::Delete { operation_id, id })
        }
        Effect::ReadClipboard { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::{
        identity::LocalPeerIdentity,
        pairing::{Invite, MAX_INVITE_BYTES},
    };

    use super::*;

    fn local() -> (PeerId, LocalPeerIdentity) {
        (PeerId::new("local").unwrap(), LocalPeerIdentity::generate())
    }

    fn invite(peer_id: &str) -> Invite {
        Invite::from_local(PeerId::new(peer_id).unwrap(), &LocalPeerIdentity::generate())
    }

    fn visible_screen() -> Screen {
        Screen { show_add_form: true, ..Screen::new() }
    }

    fn paste_new(screen: &mut Screen, local_peer_id: &PeerId, contents: Option<String>) {
        let effects =
            execute_contacts_message(screen, Message::PasteNewInvite, Some(local_peer_id));
        let [Effect::ReadClipboard { request_id, .. }] = effects.as_slice() else {
            panic!("clipboard read was not requested")
        };
        execute_contacts_message(
            screen,
            Message::NewInviteClipboardRead { request_id: *request_id, contents },
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
            Message::NewInviteChanged("not-an-invite".into()),
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
            Message::NewFingerprintConfirmed(true),
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
        let first =
            execute_contacts_message(&mut screen, Message::PasteNewInvite, Some(&local_peer_id));
        assert_eq!(first.len(), 1);
        assert!(
            execute_contacts_message(&mut screen, Message::PasteNewInvite, Some(&local_peer_id),)
                .is_empty()
        );
        let Effect::ReadClipboard { request_id, .. } = first[0] else { unreachable!() };

        execute_contacts_message(
            &mut screen,
            Message::NewInviteChanged(invite("typed").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::NewInviteClipboardRead {
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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert!(screen.new_contact_pairing.is_ready());

        let effects =
            execute_contacts_message(&mut screen, Message::PasteNewInvite, Some(&local_peer_id));

        assert_eq!(effects.len(), 1);
        assert!(!screen.new_contact_pairing.fingerprint_confirmed);
        assert!(!screen.new_contact_pairing.is_ready());
        assert!(
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
                .is_empty()
        );
    }

    #[test]
    fn clipboard_request_ids_do_not_collide_across_screen_instances() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        let old_effects = execute_contacts_message(
            &mut old_screen,
            Message::PasteNewInvite,
            Some(&local_peer_id),
        );
        let [Effect::ReadClipboard { request_id: old_request, .. }] = old_effects.as_slice() else {
            panic!("old clipboard read was not requested")
        };

        let mut new_screen = visible_screen();
        let new_effects = execute_contacts_message(
            &mut new_screen,
            Message::PasteNewInvite,
            Some(&local_peer_id),
        );
        let [Effect::ReadClipboard { request_id: new_request, .. }] = new_effects.as_slice() else {
            panic!("new clipboard read was not requested")
        };
        assert_ne!(old_request, new_request);

        execute_contacts_message(
            &mut new_screen,
            Message::NewInviteClipboardRead {
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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id));
        let request_id = ClipboardRequestId::next();
        screen.new_contact_pairing.clipboard_request = Some(request_id);

        execute_contacts_message(
            &mut screen,
            Message::NewInviteClipboardRead {
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
        let effects =
            execute_contacts_message(&mut screen, Message::PasteNewInvite, Some(&local_peer_id));
        let [Effect::ReadClipboard { request_id, .. }] = effects.as_slice() else {
            panic!("clipboard read was not requested")
        };

        execute_contacts_message(&mut screen, Message::ToggleAddForm, Some(&local_peer_id));
        execute_contacts_message(&mut screen, Message::ToggleAddForm, Some(&local_peer_id));
        execute_contacts_message(
            &mut screen,
            Message::NewInviteClipboardRead {
                request_id: *request_id,
                contents: Some(invite("stale").to_string()),
            },
            Some(&local_peer_id),
        );

        assert!(screen.show_add_form);
        assert!(screen.new_contact_pairing.invite.is_none());
        assert_eq!(
            execute_contacts_message(&mut screen, Message::PasteNewInvite, Some(&local_peer_id),)
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
            Message::NewInviteChanged("x".repeat(MAX_INVITE_BYTES + 1)),
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
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
                .is_empty()
        );

        execute_contacts_message(
            &mut screen,
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
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
        let local_invite = Invite::from_local(local_peer_id.clone(), &identity);
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
            Message::StartIdentityReplacement { id: 7, peer_id: PeerId::new("alice").unwrap() },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::ReplacementInviteChanged(invite("mallory").to_string()),
            Some(&local_peer_id),
        );
        let replacement = screen.identity_replacement.as_ref().unwrap();
        assert!(replacement.pairing.invite.is_none());
        assert!(replacement.pairing.error.as_deref().unwrap().contains("alice"));

        execute_contacts_message(
            &mut screen,
            Message::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        assert!(!screen.identity_replacement.as_ref().unwrap().pairing.fingerprint_confirmed);
        assert!(
            execute_contacts_message(
                &mut screen,
                Message::SaveIdentityReplacement,
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
            Message::StartIdentityReplacement { id: 7, peer_id: PeerId::new("alice").unwrap() },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );

        execute_contacts_message(
            &mut screen,
            Message::StartIdentityReplacement { id: 8, peer_id: PeerId::new("bob").unwrap() },
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
            Message::StartIdentityReplacement { id: 7, peer_id: PeerId::new("alice").unwrap() },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                Message::SaveIdentityReplacement,
                Some(&local_peer_id),
            )
            .len(),
            1
        );
        let operation_id =
            screen.identity_replacement.as_ref().unwrap().saving.expect("replace is pending");

        execute_contacts_message(
            &mut screen,
            Message::IdentityReplacementRejected { operation_id },
            Some(&local_peer_id),
        );

        let replacement = screen.identity_replacement.as_ref().unwrap();
        assert!(replacement.saving.is_none());
        assert!(replacement.pairing.is_ready());
        assert_eq!(
            execute_contacts_message(
                &mut screen,
                Message::SaveIdentityReplacement,
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
            Message::StartIdentityReplacement { id: 7, peer_id: PeerId::new("alice").unwrap() },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            Message::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            Message::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            Message::SaveIdentityReplacement,
            Some(&local_peer_id),
        );
        let old_operation = old_screen.identity_replacement.as_ref().unwrap().saving.unwrap();

        let mut new_screen = visible_screen();
        execute_contacts_message(
            &mut new_screen,
            Message::StartIdentityReplacement { id: 7, peer_id: PeerId::new("alice").unwrap() },
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            Message::ReplacementInviteChanged(invite("alice").to_string()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            Message::ReplacementFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            Message::SaveIdentityReplacement,
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
                Message::RequestDeleteContact(7),
                Some(&local_peer_id),
            )
            .is_empty()
        );
        assert_eq!(screen.contact_deletion.as_ref().map(|draft| draft.contact_id), Some(7));
        assert!(
            execute_contacts_message(
                &mut screen,
                Message::ConfirmDeleteContact(8),
                Some(&local_peer_id),
            )
            .is_empty()
        );
        let effects = execute_contacts_message(
            &mut screen,
            Message::ConfirmDeleteContact(7),
            Some(&local_peer_id),
        );

        assert!(matches!(effects.as_slice(), [Effect::DeleteContact { id: 7, .. }]));
        assert!(screen.contact_deletion.as_ref().unwrap().operation_id.is_some());
    }

    #[test]
    fn stale_delete_completion_cannot_clear_a_newer_confirmation() {
        let (local_peer_id, _) = local();
        let mut old_screen = visible_screen();
        execute_contacts_message(
            &mut old_screen,
            Message::RequestDeleteContact(7),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut old_screen,
            Message::ConfirmDeleteContact(7),
            Some(&local_peer_id),
        );
        let old_operation = old_screen.contact_deletion.as_ref().unwrap().operation_id.unwrap();

        let mut new_screen = visible_screen();
        execute_contacts_message(
            &mut new_screen,
            Message::RequestDeleteContact(8),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut new_screen,
            Message::ConfirmDeleteContact(8),
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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut old_screen, Message::AddNewContact, Some(&local_peer_id));
        let old_operation = old_screen.saving_new_contact.unwrap();

        let mut new_screen = visible_screen();
        new_screen.new_contact_name = "New".into();
        paste_new(&mut new_screen, &local_peer_id, Some(invite("new").to_string()));
        execute_contacts_message(
            &mut new_screen,
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut new_screen, Message::AddNewContact, Some(&local_peer_id));
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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id));

        execute_contacts_message(
            &mut screen,
            Message::NameChanged("Changed".into()),
            Some(&local_peer_id),
        );
        execute_contacts_message(
            &mut screen,
            Message::NewFingerprintConfirmed(false),
            Some(&local_peer_id),
        );
        execute_contacts_message(&mut screen, Message::ToggleAddForm, Some(&local_peer_id));

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
            Message::NewFingerprintConfirmed(true),
            Some(&local_peer_id),
        );
        assert_eq!(
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
                .len(),
            1
        );
        let operation_id = screen.saving_new_contact.expect("save is pending");

        execute_contacts_message(
            &mut screen,
            Message::NewContactSaveRejected { operation_id },
            Some(&local_peer_id),
        );

        assert!(screen.saving_new_contact.is_none());
        assert_eq!(screen.new_contact_name, "Alice");
        assert!(screen.new_contact_pairing.is_ready());
        assert_eq!(
            execute_contacts_message(&mut screen, Message::AddNewContact, Some(&local_peer_id),)
                .len(),
            1
        );
    }
}
