use std::net::SocketAddr;

use iced::Task;

use super::{MessagesScreen, workflow};
use crate::ui::{
    message::{Message, MessagingServiceMessage, NotificationMessage, ScreenMessage},
    shell::{AppContext, AppContextMut},
};

impl MessagesScreen {
    pub(crate) fn handle_message(
        &mut self,
        ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        let effects = match message {
            Message::Screen(ScreenMessage::Messages(message)) => {
                workflow::execute_messages_message(
                    self,
                    ctx.messaging.active_peer_id.as_deref(),
                    message,
                )
            }
            _ => return Task::none(),
        };

        Task::batch(effects.into_iter().map(|effect| match effect {
            workflow::MessagesEffect::SendMessage { peer_id, body } => {
                match resolve_peer_address(ctx.as_ref(), &peer_id) {
                    Ok(address) => {
                        Task::done(Message::Messaging(MessagingServiceMessage::SendMessage {
                            peer_id,
                            address,
                            body,
                        }))
                    }
                    Err(message) => {
                        Task::done(Message::Notification(NotificationMessage::NotifyError(message)))
                    }
                }
            }
        }))
    }
}

fn resolve_peer_address(ctx: AppContext<'_>, peer_id: &str) -> Result<SocketAddr, String> {
    if let Some(peer) = ctx.networking.discovered_peers.iter().find(|peer| peer.id == peer_id)
        && let Some(address) = peer.addresses.first()
    {
        return Ok(SocketAddr::new(*address, peer.port));
    }

    if let Some(contact) = ctx.contacts.contacts.iter().find(|contact| contact.peer_id == peer_id)
        && let Some(address) = &contact.address
    {
        return address
            .parse::<SocketAddr>()
            .map_err(|_| format!("Saved address for {} is invalid.", contact.name));
    }

    Err("Peer is not currently reachable. Wait for discovery or save a valid address.".into())
}
