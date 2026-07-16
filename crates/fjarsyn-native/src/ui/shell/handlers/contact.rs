use std::sync::Arc;

use iced::Task;

use crate::ui::{
    message::{ContactsServiceMessage, Message, ScreenMessage},
    presentation::project_peer,
    screens::contacts::ContactsMessage,
    shell::Fjarsyn,
};

pub fn handle_contact_msg(app: &mut Fjarsyn, message: ContactsServiceMessage) -> Task<Message> {
    match message {
        ContactsServiceMessage::LoadContacts => refresh_task(app),
        ContactsServiceMessage::SaveContact { operation_id, name, identity } => {
            let Some(service) = contacts_service(app) else {
                app.ctx.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_save_task(ContactsMessage::NewContactSaveRejected {
                    operation_id,
                });
            };
            Task::future(async move {
                let result = service.create(name, identity).await.map_err(Arc::new);
                Message::ContactData(ContactsServiceMessage::ContactSaved { operation_id, result })
            })
        }
        ContactsServiceMessage::DeleteContact { operation_id, id } => {
            let Some(contact) = app.ctx.contacts.iter().find(|contact| contact.id == id) else {
                app.ctx.notify_error("Contact no longer exists.");
                return rejected_save_task(ContactsMessage::DeleteContactRejected {
                    operation_id,
                    id,
                });
            };
            let phase =
                app.ctx.sessions.session_for_peer(&contact.peer_id).map(|session| session.phase);
            if !project_peer(false, phase).can_mutate_trust() {
                app.ctx
                    .notify_error("Disconnect this contact before deleting its trusted identity.");
                return rejected_save_task(ContactsMessage::DeleteContactRejected {
                    operation_id,
                    id,
                });
            }
            let Some(service) = contacts_service(app) else {
                app.ctx.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_save_task(ContactsMessage::DeleteContactRejected {
                    operation_id,
                    id,
                });
            };
            Task::future(async move {
                let result = service.delete(id).await.map_err(Arc::new);
                Message::ContactData(ContactsServiceMessage::ContactDeleted {
                    operation_id,
                    id,
                    result,
                })
            })
        }
        ContactsServiceMessage::UpdateContactVerifiedIdentity { operation_id, id, identity } => {
            let Some(service) = contacts_service(app) else {
                app.ctx.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_save_task(ContactsMessage::IdentityReplacementRejected {
                    operation_id,
                });
            };
            let Some(contact) = app.ctx.contacts.iter().find(|contact| contact.id == id).cloned()
            else {
                app.ctx.notify_error("Contact no longer exists.");
                return rejected_save_task(ContactsMessage::IdentityReplacementRejected {
                    operation_id,
                });
            };
            let phase =
                app.ctx.sessions.session_for_peer(&contact.peer_id).map(|session| session.phase);
            if !project_peer(false, phase).can_mutate_trust() {
                app.ctx.notify_error(
                    "Disconnect this contact before replacing its verified identity.",
                );
                return rejected_save_task(ContactsMessage::IdentityReplacementRejected {
                    operation_id,
                });
            }
            Task::future(async move {
                let result = service.update_verified_identity(id, identity).await.map_err(Arc::new);
                Message::ContactData(ContactsServiceMessage::ContactUpdated {
                    operation_id,
                    result,
                })
            })
        }
        ContactsServiceMessage::ContactsLoaded(result) => {
            apply_contacts_result(app, result);
            Task::none()
        }
        ContactsServiceMessage::ContactSaved { result, .. } => {
            apply_mutation_result(app, result, "Contact saved.");
            Task::none()
        }
        ContactsServiceMessage::ContactDeleted { result, .. } => {
            apply_mutation_result(app, result, "Contact deleted.");
            Task::none()
        }
        ContactsServiceMessage::ContactUpdated { result, .. } => {
            apply_mutation_result(app, result, "Contact identity updated.");
            Task::none()
        }
    }
}

fn contacts_service(
    app: &Fjarsyn,
) -> Option<Arc<fjarsyn_core::services::contact_trust_service::ContactTrustService>> {
    app.runtime.application.as_ref().map(|runtime| runtime.handles.contacts.clone())
}

fn refresh_task(app: &mut Fjarsyn) -> Task<Message> {
    let Some(service) = contacts_service(app) else {
        return unavailable(app);
    };
    Task::future(async move {
        let result = service.refresh().await.map_err(Arc::new);
        Message::ContactData(ContactsServiceMessage::ContactsLoaded(result))
    })
}

fn unavailable(app: &mut Fjarsyn) -> Task<Message> {
    app.ctx.notify_error("Contacts are unavailable while Fjarsyn is starting.");
    Task::none()
}

fn rejected_save_task(message: ContactsMessage) -> Task<Message> {
    Task::done(Message::Screen(ScreenMessage::Contacts(message)))
}

fn apply_contacts_result(
    app: &mut Fjarsyn,
    result: Result<
        fjarsyn_core::services::contact_trust_service::ContactRefreshOutcome,
        Arc<fjarsyn_core::services::contact_trust_service::ContactTrustError>,
    >,
) {
    match result {
        Ok(outcome) => {
            apply_contact_projection(app, outcome.projection);
            for warning in refresh_admission_warning_messages(&outcome.admission_warnings) {
                app.ctx.notify_error(warning);
            }
        }
        Err(error) => app.ctx.notify_error(error.to_string()),
    }
}

fn refresh_admission_warning_messages(
    warnings: &[fjarsyn_core::services::contact_trust_service::PeerAdmissionWarning],
) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| {
            format!(
                "Peer {} remains suspended after refreshing contacts: {}",
                warning.peer_id, warning.error
            )
        })
        .collect()
}

fn apply_mutation_result(
    app: &mut Fjarsyn,
    result: Result<
        fjarsyn_core::services::contact_trust_service::ContactTrustOutcome,
        Arc<fjarsyn_core::services::contact_trust_service::ContactTrustError>,
    >,
    success: &str,
) {
    match result {
        Ok(outcome) => {
            apply_contact_projection(app, outcome.projection);
            app.ctx.notify_success(success);
            if let Some(warning) = outcome.admission_warning {
                app.ctx.notify_error(format!(
                    "The contact change was saved, but peer sessions remain suspended: {warning}"
                ));
            }
        }
        Err(error) => app.ctx.notify_error(error.to_string()),
    }
}

fn apply_contact_projection(
    app: &mut Fjarsyn,
    projection: fjarsyn_core::services::contacts_service::ContactProjection,
) {
    if accept_contact_revision(
        app.ctx.contacts_source_id,
        &mut app.ctx.contacts_revision,
        projection.source_id,
        projection.revision,
    ) {
        app.ctx.contacts = projection.contacts;
    }
}

fn accept_contact_revision(
    current_source_id: u64,
    current_revision: &mut u64,
    incoming_source_id: u64,
    incoming_revision: u64,
) -> bool {
    if incoming_source_id != current_source_id || incoming_revision <= *current_revision {
        return false;
    }
    *current_revision = incoming_revision;
    true
}

#[cfg(test)]
mod tests {
    use fjarsyn_core::{
        peer_session::{PeerId, PeerSessionError},
        services::contact_trust_service::PeerAdmissionWarning,
    };

    use super::{accept_contact_revision, refresh_admission_warning_messages};

    #[test]
    fn newer_then_older_contact_completion_cannot_roll_back_projection() {
        let mut revision = 3;

        assert!(accept_contact_revision(7, &mut revision, 7, 5));
        assert_eq!(revision, 5);
        assert!(!accept_contact_revision(7, &mut revision, 7, 4));
        assert_eq!(revision, 5);
        assert!(!accept_contact_revision(7, &mut revision, 7, 5));
    }

    #[test]
    fn old_runtime_source_cannot_override_a_new_runtime_projection() {
        let current_source = 2;
        let mut current_revision = 1;

        assert!(!accept_contact_revision(current_source, &mut current_revision, 1, 99,));
        assert_eq!(current_revision, 1);
    }

    #[test]
    fn contact_refresh_surfaces_every_admission_warning() {
        let warnings = [
            PeerAdmissionWarning {
                peer_id: PeerId::new("alice").unwrap(),
                error: PeerSessionError::ServiceStopped,
            },
            PeerAdmissionWarning {
                peer_id: PeerId::new("bob").unwrap(),
                error: PeerSessionError::OperationTimeout,
            },
        ];

        let messages = refresh_admission_warning_messages(&warnings);

        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            "Peer alice remains suspended after refreshing contacts: the peer-session service has stopped"
        );
        assert_eq!(
            messages[1],
            "Peer bob remains suspended after refreshing contacts: peer-session operation timed out"
        );
    }
}
