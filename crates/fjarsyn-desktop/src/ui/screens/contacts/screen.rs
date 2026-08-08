use fjarsyn_engine::identity::PeerId;
use iced::Task;

use super::{PairingDraft, workflow};
use crate::ui::{
    message::{
        self,
        screen::contacts::{Message as ContactsMessage, OperationId},
    },
    presentation::Context,
};

#[derive(Debug, Clone)]
pub(super) struct DeletionDraft {
    pub(super) contact_id: i64,
    pub(super) operation_id: Option<OperationId>,
}

#[derive(Debug, Clone)]
pub(super) struct IdentityReplacementDraft {
    pub(super) contact_id: i64,
    pub(super) expected_peer_id: PeerId,
    pub(super) pairing: PairingDraft,
    pub(super) saving: Option<OperationId>,
}

#[derive(Debug, Clone)]
pub(in crate::ui::screens) struct Screen {
    pub(super) new_contact_name: String,
    pub(super) new_contact_pairing: Box<PairingDraft>,
    pub(super) saving_new_contact: Option<OperationId>,
    pub(super) identity_replacement: Option<Box<IdentityReplacementDraft>>,
    pub(super) contact_deletion: Option<DeletionDraft>,
    pub(super) show_add_form: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub(in crate::ui::screens) fn new() -> Self {
        Self {
            new_contact_name: String::new(),
            new_contact_pairing: Box::new(PairingDraft::default()),
            saving_new_contact: None,
            identity_replacement: None,
            contact_deletion: None,
            show_add_form: false,
        }
    }

    fn handle_message(
        &mut self,
        context: Context<'_>,
        message: message::Message,
    ) -> Task<message::Message> {
        let effects = match message {
            message::Message::Screen(message::Screen::Contacts(message)) => {
                workflow::execute_contacts_message(self, message, context.local_peer_id())
            }
            message::Message::ContactOperation(message::ContactOperation::Saved {
                operation_id,
                ref result,
            }) => {
                workflow::finish_contact_save(self, operation_id, result.is_ok());
                return Task::none();
            }
            message::Message::ContactOperation(message::ContactOperation::Updated {
                operation_id,
                ref result,
            }) => {
                workflow::finish_identity_replacement(self, operation_id, result.is_ok());
                return Task::none();
            }
            message::Message::ContactOperation(message::ContactOperation::Deleted {
                operation_id,
                id,
                ref result,
            }) => {
                workflow::finish_contact_delete(self, operation_id, id, result.is_ok());
                return Task::none();
            }
            _ => return Task::none(),
        };

        Task::batch(effects.into_iter().map(effect_task))
    }
}

fn effect_task(effect: workflow::Effect) -> Task<message::Message> {
    match effect {
        workflow::Effect::ReadClipboard { target, request_id } => {
            iced::clipboard::read().map(move |contents| {
                let message = match target {
                    workflow::ClipboardTarget::NewContact => {
                        ContactsMessage::NewInviteClipboardRead { request_id, contents }
                    }
                    workflow::ClipboardTarget::IdentityReplacement => {
                        ContactsMessage::ReplacementInviteClipboardRead { request_id, contents }
                    }
                };
                message::Message::Screen(message::Screen::Contacts(message))
            })
        }
        effect => workflow::into_contact_operation(effect)
            .map(|operation| Task::done(message::Message::ContactOperation(operation)))
            .unwrap_or_else(Task::none),
    }
}

impl crate::ui::screens::Screen for Screen {
    fn update(
        &mut self,
        context: Context<'_>,
        message: message::Message,
    ) -> Task<message::Message> {
        self.handle_message(context, message)
    }

    fn view<'a>(&'a self, context: Context<'a>) -> iced::Element<'a, message::Message> {
        self.render_view(context)
    }
}
