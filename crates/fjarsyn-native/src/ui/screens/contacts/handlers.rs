use iced::Task;

use super::{ContactsMessage, ContactsScreen, workflow};
use crate::ui::{
    message::{ContactsServiceMessage, Message, ScreenMessage},
    shell::ShellContextMut,
};

impl ContactsScreen {
    pub(crate) fn handle_message(
        &mut self,
        ctx: &mut ShellContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        let effects = match message {
            Message::Screen(ScreenMessage::Contacts(message)) => {
                workflow::execute_contacts_message(self, message, ctx.local_peer_id.as_ref())
            }
            Message::ContactData(ContactsServiceMessage::ContactSaved {
                operation_id,
                ref result,
            }) => {
                workflow::finish_contact_save(self, operation_id, result.is_ok());
                return Task::none();
            }
            Message::ContactData(ContactsServiceMessage::ContactUpdated {
                operation_id,
                ref result,
            }) => {
                workflow::finish_identity_replacement(self, operation_id, result.is_ok());
                return Task::none();
            }
            Message::ContactData(ContactsServiceMessage::ContactDeleted {
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

fn effect_task(effect: workflow::ContactsEffect) -> Task<Message> {
    match effect {
        workflow::ContactsEffect::ReadClipboard { target, request_id } => iced::clipboard::read()
            .map(move |contents| {
                let message = match target {
                    workflow::ClipboardTarget::NewContact => {
                        ContactsMessage::NewInviteClipboardRead { request_id, contents }
                    }
                    workflow::ClipboardTarget::IdentityReplacement => {
                        ContactsMessage::ReplacementInviteClipboardRead { request_id, contents }
                    }
                };
                Message::Screen(ScreenMessage::Contacts(message))
            }),
        effect => workflow::into_service_message(effect)
            .map(|message| Task::done(Message::ContactData(message)))
            .unwrap_or_else(Task::none),
    }
}
