use std::collections::HashMap;

use tokio::sync::{RwLock, mpsc};

use crate::networking::protocol::SignalingMessage;

#[derive(Default)]
pub(super) struct PeerRoutes {
    routes: RwLock<HashMap<String, PeerRoute>>,
}

struct PeerRoute {
    connection_id: u64,
    sender: mpsc::Sender<SignalingMessage>,
}

pub(super) async fn route_signaling_message(peer_routes: &PeerRoutes, message: SignalingMessage) {
    if let Some(peer_id) = message.to.clone() {
        let route = {
            let routes = peer_routes.routes.read().await;
            routes.get(&peer_id).map(|route| route.sender.clone())
        };

        if let Some(route) = route {
            let _ = route.send(message).await;
        } else {
            tracing::debug!("No signaling route found for peer {}", peer_id);
        }
        return;
    }

    let routes = {
        let routes = peer_routes.routes.read().await;
        routes.values().map(|route| route.sender.clone()).collect::<Vec<_>>()
    };

    for route in routes {
        let _ = route.send(message.clone()).await;
    }
}

pub(super) async fn register_peer_route(
    peer_routes: &PeerRoutes,
    connection_id: u64,
    connection_tx: mpsc::Sender<SignalingMessage>,
    peer_id: &str,
    registered_peer_id: &mut Option<String>,
) -> bool {
    if !connection_peer_matches_message(registered_peer_id.as_deref(), peer_id) {
        tracing::warn!(
            "Ignoring signaling message that tried to change connection identity from {:?} to {}.",
            registered_peer_id,
            peer_id
        );
        return false;
    }

    let mut routes = peer_routes.routes.write().await;
    routes.insert(peer_id.to_string(), PeerRoute { connection_id, sender: connection_tx });
    *registered_peer_id = Some(peer_id.to_string());
    true
}

pub(super) async fn unregister_peer_route(
    peer_routes: &PeerRoutes,
    connection_id: u64,
    peer_id: Option<&str>,
) {
    let Some(peer_id) = peer_id else {
        return;
    };

    let mut routes = peer_routes.routes.write().await;
    if routes.get(peer_id).is_some_and(|route| route.connection_id == connection_id) {
        routes.remove(peer_id);
    }
}

fn connection_peer_matches_message(
    registered_peer_id: Option<&str>,
    message_peer_id: &str,
) -> bool {
    match registered_peer_id {
        Some(peer_id) => peer_id == message_peer_id,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregistered_connection_accepts_first_peer_id() {
        assert!(connection_peer_matches_message(None, "peer-a"));
    }

    #[test]
    fn registered_connection_accepts_matching_peer_id() {
        assert!(connection_peer_matches_message(Some("peer-a"), "peer-a"));
    }

    #[test]
    fn registered_connection_rejects_peer_id_change() {
        assert!(!connection_peer_matches_message(Some("peer-a"), "peer-b"));
    }
}
