use fjarsyn_core::{
    communication::call::CallTarget,
    networking::discovery::PeerInfo,
    services::call_service::{CallState, DialSuccess},
};
use iced::Task;

use crate::ui::{
    message::{
        CallActionMessage, CallServiceMessage, ContactsServiceMessage, Message, NavigationMessage,
    },
    shell::Fjarsyn,
};

pub(super) fn run_accept_call(app: &mut Fjarsyn) -> Task<Message> {
    let Some(service) = app.runtime.services.call_service.clone() else {
        return Task::none();
    };
    let restore_peer_id = match service.state() {
        CallState::IncomingCall { peer_id } => Some(peer_id),
        _ => None,
    };

    Task::future(async move {
        match service.accept().await {
            Ok(_) => {
                Message::Navigation(NavigationMessage::Navigate(crate::ui::message::Route::Call))
            }
            Err(err) => Message::CallAction(CallActionMessage::AcceptFailed {
                error: err.to_string(),
                peer_id: restore_peer_id,
            }),
        }
    })
}

pub(super) fn run_decline_call(app: &mut Fjarsyn) -> Task<Message> {
    let Some(service) = app.runtime.services.call_service.clone() else {
        return Task::none();
    };
    let restore_peer_id = match service.state() {
        CallState::IncomingCall { peer_id } => Some(peer_id),
        _ => None,
    };

    Task::future(async move {
        match service.decline().await {
            Ok(_) => Message::NoOp,
            Err(err) => Message::CallAction(CallActionMessage::DeclineFailed {
                error: err.to_string(),
                peer_id: restore_peer_id,
            }),
        }
    })
}

pub(super) fn run_start_call(app: &mut Fjarsyn, target: CallTarget) -> Task<Message> {
    let Some(service) = app.runtime.services.call_service.clone() else {
        return Task::none();
    };
    let contacts = app.ctx.contacts.contacts.clone();
    let discovered = app.ctx.networking.discovered_peers.clone();

    Task::future(async move {
        service
            .dial(target, &contacts, &discovered)
            .await
            .map(|DialSuccess { peer_id, name, socket_addr, update_contact_address }| {
                let mut batch = Vec::new();

                if let Some((id, addr)) = update_contact_address {
                    batch.push(Message::ContactData(
                        ContactsServiceMessage::UpdateContactAddress { id, new_address: addr },
                    ));
                }

                if let (Some(id), Some(name), Some(addr)) = (peer_id, name, socket_addr) {
                    batch.push(Message::CallService(CallServiceMessage::PeerFound(PeerInfo {
                        id,
                        instance_name: name,
                        host_name: "direct".into(),
                        addresses: vec![addr.ip()],
                        port: addr.port(),
                    })));
                }

                batch.push(Message::Navigation(NavigationMessage::Navigate(
                    crate::ui::message::Route::Call,
                )));
                Message::Batch(batch)
            })
            .unwrap_or_else(|err| Message::CallAction(CallActionMessage::StartFailed(err)))
    })
}
