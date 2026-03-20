use iced::Task;

use crate::{
    networking::discovery::PeerInfo,
    services::call_service::DialSuccess,
    ui::{
        app::{
            Fjarsyn,
            workflows::call_action::{self, CallActionEffect},
        },
        message::{
            CallActionMessage, CallServiceMessage, ContactsServiceMessage, Message,
            NavigationMessage, Route,
        },
        utils::{ErrorExt, ResultExt},
    },
};

pub fn handle_call_action_msg(app: &mut Fjarsyn, message: CallActionMessage) -> Task<Message> {
    let effects = call_action::reduce(app, message);
    Task::batch(effects.into_iter().map(run_effect))
}

fn run_effect(effect: CallActionEffect) -> Task<Message> {
    match effect {
        CallActionEffect::Accept(service) => Task::future(async move {
            service
                .accept()
                .await
                .map(|_| Message::Navigation(NavigationMessage::Navigate(Route::Call)))
                .unwrap_or_else(|err| err.to_notify_error())
        }),
        CallActionEffect::Decline(service) => Task::future(async move {
            service
                .decline()
                .await
                .map(|_| Message::NoOp)
                .unwrap_or_else(|err| err.to_notify_error())
        }),
        CallActionEffect::Start { service, target, contacts, discovered } => {
            Task::future(async move {
                service
                    .dial(target, &contacts, &discovered)
                    .await
                    .map(|DialSuccess { peer_id, name, socket_addr, update_contact_address }| {
                        let mut batch = Vec::new();

                        if let Some((id, addr)) = update_contact_address {
                            batch.push(Message::ContactData(
                                ContactsServiceMessage::UpdateContactAddress {
                                    id,
                                    new_address: addr,
                                },
                            ));
                        }

                        if let (Some(id), Some(name), Some(addr)) = (peer_id, name, socket_addr) {
                            batch.push(Message::CallService(CallServiceMessage::PeerFound(
                                PeerInfo {
                                    id,
                                    instance_name: name,
                                    host_name: "direct".into(),
                                    addresses: vec![addr.ip()],
                                    port: addr.port(),
                                },
                            )));
                        }

                        batch.push(Message::Navigation(NavigationMessage::Navigate(Route::Call)));
                        Message::Batch(batch)
                    })
                    .map_notify_error()
                    .unwrap_or_else(|message| message)
            })
        }
    }
}
