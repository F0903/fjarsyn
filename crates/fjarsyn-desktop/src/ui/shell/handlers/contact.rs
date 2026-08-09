use std::sync::Arc;

use iced::Task;

use crate::ui::{
    message::{self, Message},
    presentation::project_peer,
    shell::Fjarsyn,
};

pub(in crate::ui::shell) fn handle_contact_operation(
    app: &mut Fjarsyn,
    operation: message::ContactOperation,
) -> Task<Message> {
    match operation {
        message::ContactOperation::Save { operation_id, name, identity } => {
            let Some(service) = contact_service(app) else {
                app.state.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_operation_task(
                    message::screen::contacts::Message::NewContactSaveRejected { operation_id },
                );
            };
            Task::future(async move {
                let result = service.create(name, identity).await.map_err(Arc::new);
                Message::ContactOperation(message::ContactOperation::Saved { operation_id, result })
            })
        }
        message::ContactOperation::Delete { operation_id, id } => {
            let Some(contact) = app.state.contacts().iter().find(|contact| contact.id == id) else {
                app.state.notify_error("Contact no longer exists.");
                return rejected_operation_task(
                    message::screen::contacts::Message::DeleteContactRejected { operation_id, id },
                );
            };
            let phase =
                app.state.sessions.session_for_peer(&contact.peer_id).map(|session| session.phase);
            if !project_peer(false, phase).can_mutate_trust() {
                app.state
                    .notify_error("Disconnect this contact before deleting its trusted identity.");
                return rejected_operation_task(
                    message::screen::contacts::Message::DeleteContactRejected { operation_id, id },
                );
            }
            let Some(service) = contact_service(app) else {
                app.state.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_operation_task(
                    message::screen::contacts::Message::DeleteContactRejected { operation_id, id },
                );
            };
            Task::future(async move {
                let result = service.delete(id).await.map_err(Arc::new);
                Message::ContactOperation(message::ContactOperation::Deleted {
                    operation_id,
                    id,
                    result,
                })
            })
        }
        message::ContactOperation::UpdateVerifiedIdentity { operation_id, id, identity } => {
            let Some(service) = contact_service(app) else {
                app.state.notify_error("Contacts are unavailable while Fjarsyn is starting.");
                return rejected_operation_task(
                    message::screen::contacts::Message::IdentityReplacementRejected {
                        operation_id,
                    },
                );
            };
            let Some(contact) =
                app.state.contacts().iter().find(|contact| contact.id == id).cloned()
            else {
                app.state.notify_error("Contact no longer exists.");
                return rejected_operation_task(
                    message::screen::contacts::Message::IdentityReplacementRejected {
                        operation_id,
                    },
                );
            };
            let phase =
                app.state.sessions.session_for_peer(&contact.peer_id).map(|session| session.phase);
            if !project_peer(false, phase).can_mutate_trust() {
                app.state.notify_error(
                    "Disconnect this contact before replacing its verified identity.",
                );
                return rejected_operation_task(
                    message::screen::contacts::Message::IdentityReplacementRejected {
                        operation_id,
                    },
                );
            }
            Task::future(async move {
                let result = service.update_verified_identity(id, identity).await.map_err(Arc::new);
                Message::ContactOperation(message::ContactOperation::Updated {
                    operation_id,
                    result,
                })
            })
        }
        message::ContactOperation::Saved { operation_id, result } => {
            app.active_screen.contact_save_finished(operation_id, result.is_ok());
            apply_mutation_result(app, result, "Contact saved.");
            Task::none()
        }
        message::ContactOperation::Deleted { operation_id, id, result } => {
            app.active_screen.contact_delete_finished(operation_id, id, result.is_ok());
            apply_mutation_result(app, result, "Contact deleted.");
            Task::none()
        }
        message::ContactOperation::Updated { operation_id, result } => {
            app.active_screen.contact_identity_update_finished(operation_id, result.is_ok());
            apply_mutation_result(app, result, "Contact identity updated.");
            Task::none()
        }
    }
}

fn contact_service(app: &Fjarsyn) -> Option<fjarsyn_engine::contacts::ContactsService> {
    app.runtime.engine.as_ref().map(|runtime| runtime.contacts().clone())
}

fn rejected_operation_task(message: message::screen::contacts::Message) -> Task<Message> {
    Task::done(Message::Screen(message::Screen::Contacts(message)))
}

fn apply_mutation_result(
    app: &mut Fjarsyn,
    result: Result<fjarsyn_engine::contacts::Outcome, Arc<fjarsyn_engine::contacts::Error>>,
    success: &str,
) {
    match result {
        Ok(outcome) => {
            accept_contact_projection(&mut app.state.contact_projection, outcome.projection);
            app.state.notify_success(success);
            if let Some(warning) = outcome.admission_warning {
                app.state.notify_error(format!(
                    "The contact change was saved, but peer sessions remain suspended: {warning}"
                ));
            }
        }
        Err(error) => app.state.notify_error(error.to_string()),
    }
}

fn accept_contact_projection(
    current: &mut Option<fjarsyn_engine::contacts::Projection>,
    incoming: fjarsyn_engine::contacts::Projection,
) -> bool {
    let Some(active) = current.as_ref() else {
        return false;
    };
    if incoming.source_id != active.source_id || incoming.revision <= active.revision {
        return false;
    }
    *current = Some(incoming);
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fjarsyn_engine::contacts::Projection;

    use super::accept_contact_projection;

    fn projection(source_id: u64, revision: u64) -> Projection {
        Projection { contacts: Arc::new(Vec::new()), source_id, revision }
    }

    #[test]
    fn newer_then_older_contact_completion_cannot_roll_back_projection() {
        let mut current = Some(projection(7, 3));

        assert!(accept_contact_projection(&mut current, projection(7, 5)));
        assert_eq!(current.as_ref().unwrap().revision, 5);
        assert!(!accept_contact_projection(&mut current, projection(7, 4)));
        assert_eq!(current.as_ref().unwrap().revision, 5);
        assert!(!accept_contact_projection(&mut current, projection(7, 5)));
    }

    #[test]
    fn old_runtime_source_cannot_override_a_new_runtime_projection() {
        let mut current = Some(projection(2, 1));

        assert!(!accept_contact_projection(&mut current, projection(1, 99)));
        assert_eq!(current.as_ref().unwrap().revision, 1);
    }

    #[test]
    fn mutation_completion_cannot_install_a_projection_before_runtime_initialization() {
        let mut current = None;

        assert!(!accept_contact_projection(&mut current, projection(1, 1)));
        assert!(current.is_none());
    }
}
