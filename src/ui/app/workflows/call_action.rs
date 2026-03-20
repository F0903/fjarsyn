use std::sync::Arc;

use crate::{
    networking::discovery::PeerInfo,
    services::{call_service::CallService, contacts_service::Contact},
    ui::{
        app::Fjarsyn,
        message::{CallActionMessage, CallTarget},
    },
};

pub(crate) enum CallActionEffect {
    Accept(Arc<CallService>),
    Decline(Arc<CallService>),
    Start {
        service: Arc<CallService>,
        target: CallTarget,
        contacts: Arc<Vec<Contact>>,
        discovered: Vec<PeerInfo>,
    },
}

// Keep call-action decisions here so the Iced-facing handler only has to run the
// resulting async work.
pub(crate) fn reduce(app: &mut Fjarsyn, message: CallActionMessage) -> Vec<CallActionEffect> {
    match message {
        CallActionMessage::AcceptCall => reduce_accept_call(app),
        CallActionMessage::DeclineCall => reduce_decline_call(app),
        CallActionMessage::StartCall(target) => reduce_start_call(app, target),
    }
}

fn reduce_accept_call(app: &mut Fjarsyn) -> Vec<CallActionEffect> {
    app.ctx.session.incoming_call_id = None;
    app.ctx.session.incoming_call_timeout = None;
    app.ctx.session.call_connected = false;

    app.ctx.services.call_service.clone().map(CallActionEffect::Accept).into_iter().collect()
}

fn reduce_decline_call(app: &mut Fjarsyn) -> Vec<CallActionEffect> {
    app.ctx.session.target_id = None;
    app.ctx.session.target_label = None;
    app.ctx.session.incoming_call_id = None;
    app.ctx.session.incoming_call_timeout = None;
    app.ctx.session.call_connected = false;

    app.ctx.services.call_service.clone().map(CallActionEffect::Decline).into_iter().collect()
}

fn reduce_start_call(app: &mut Fjarsyn, target: CallTarget) -> Vec<CallActionEffect> {
    app.ctx.session.call_connected = false;

    let Some(service) = app.ctx.services.call_service.clone() else {
        return Vec::new();
    };
    let Some(contacts_service) = app.ctx.services.contacts_service.clone() else {
        return Vec::new();
    };

    let contacts = contacts_service.contacts();
    let discovered = app.ctx.networking.discovered_peers.clone();
    let (target_id, target_label) = resolve_call_target_hint(&target, &contacts, &discovered);

    app.ctx.session.target_id = target_id;
    app.ctx.session.target_label = target_label;
    app.ctx.session.incoming_call_id = None;
    app.ctx.session.incoming_call_timeout = None;

    vec![CallActionEffect::Start { service, target, contacts, discovered }]
}

fn resolve_call_target_hint(
    target: &CallTarget,
    contacts: &[Contact],
    discovered: &[PeerInfo],
) -> (Option<String>, Option<String>) {
    match target {
        CallTarget::PeerId(id) => {
            let label = discovered
                .iter()
                .find(|peer| peer.id == *id)
                .and_then(|peer| non_empty(peer.instance_name.clone()));
            (Some(id.clone()), label)
        }
        CallTarget::Address(addr) => (None, Some(addr.clone())),
        CallTarget::ContactId(id) => contacts
            .iter()
            .find(|contact| contact.id == *id)
            .map(|contact| {
                (
                    Some(contact.peer_id.clone()),
                    non_empty(contact.name.clone()).or_else(|| contact.address.clone()),
                )
            })
            .unwrap_or((None, None)),
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}
