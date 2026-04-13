use crate::{
    communication::{call::CallTarget, messaging::ConversationSummary},
    networking::discovery::PeerInfo,
    services::contacts_service::Contact,
    utils::text,
};

pub fn resolve_selected_peer_id(
    summaries: &[ConversationSummary],
    selected_peer_id: Option<String>,
) -> Option<String> {
    selected_peer_id.or_else(|| summaries.first().map(|summary| summary.peer_id.clone()))
}

pub fn resolve_call_target_hint(
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

pub fn peer_label(contacts: &[Contact], discovered: &[PeerInfo], peer_id: &str) -> String {
    if let Some(contact) = contacts.iter().find(|contact| contact.peer_id == peer_id) {
        return contact.name.clone();
    }

    discovered
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| text::truncate(peer_id, 12).to_string())
}

pub fn peer_display_name(
    contacts: &[Contact],
    discovered: &[PeerInfo],
    peer_id: &str,
    max_chars: usize,
) -> String {
    if let Some(contact) = contacts.iter().find(|contact| contact.peer_id == peer_id) {
        return text::truncate_with_ellipsis(contact.name.trim(), max_chars);
    }

    discovered
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| text::truncate_with_ellipsis(&name, max_chars))
        .unwrap_or_else(|| text::abbreviate_middle(peer_id, 14, 6))
}

pub fn message_preview(body: &str, max_chars: usize) -> String {
    if body.chars().count() <= max_chars {
        body.to_string()
    } else {
        format!("{}...", body.chars().take(max_chars).collect::<String>())
    }
}

pub fn update_recent_peer(
    recent_peers: &mut Vec<PeerInfo>,
    discovered_peers: &[PeerInfo],
    target_id: Option<&str>,
) {
    if let Some(target_id) = target_id
        && let Some(peer) = discovered_peers.iter().find(|peer| peer.id == target_id).cloned()
    {
        recent_peers.retain(|recent| recent.id != peer.id);
        recent_peers.insert(0, peer);
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}
