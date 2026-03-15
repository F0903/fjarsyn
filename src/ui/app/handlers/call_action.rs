use iced::Task;

use crate::{
    networking::discovery::PeerInfo,
    services::call_service::DialSuccess,
    ui::{
        app::Fjarsyn,
        message::{
            CallActionMessage, CallServiceMessage, ContactsServiceMessage, Message,
            NavigationMessage, Route,
        },
        utils::ResultExt,
    },
};

pub fn handle_call_action_msg(app: &mut Fjarsyn, msg: CallActionMessage) -> Task<Message> {
    match msg {
        CallActionMessage::AcceptCall => {
            app.ctx.session.incoming_call_id = None;
            app.ctx.session.incoming_call_timeout = None;
            let Some(service) = app.ctx.services.call_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                match service.accept().await {
                    Ok(_) => Message::Navigation(NavigationMessage::Navigate(Route::Call)),
                    Err(_) => Message::NoOp,
                }
            })
        }
        CallActionMessage::DeclineCall => {
            app.ctx.session.incoming_call_id = None;
            app.ctx.session.incoming_call_timeout = None;
            let Some(service) = app.ctx.services.call_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                let _ = service.decline().await;
                Message::NoOp
            })
        }
        CallActionMessage::StartCall(target) => {
            let Some(service) = app.ctx.services.call_service.clone() else {
                return Task::none();
            };
            let Some(contacts_service) = app.ctx.services.contacts_service.clone() else {
                return Task::none();
            };
            let target = target.clone();
            let discovered = app.ctx.networking.discovered_peers.clone();
            let contacts = contacts_service.contacts();

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
                    .unwrap_or_else(|msg| msg)
            })
        }
    }
}
