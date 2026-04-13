use tokio::sync::mpsc;

use super::PeerRouteMap;
use crate::networking::protocol::SignalingMessage;

pub(super) async fn route_signaling_message(peer_routes: &PeerRouteMap, message: SignalingMessage) {
    if let Some(peer_id) = message.to.clone() {
        let route = {
            let routes = peer_routes.read().await;
            routes.get(&peer_id).map(|(_, sender)| sender.clone())
        };

        if let Some(route) = route {
            let _ = route.send(message).await;
        } else {
            tracing::debug!("No signaling route found for peer {}", peer_id);
        }
        return;
    }

    let routes = {
        let routes = peer_routes.read().await;
        routes.values().map(|(_, sender)| sender.clone()).collect::<Vec<_>>()
    };

    for route in routes {
        let _ = route.send(message.clone()).await;
    }
}

pub(super) async fn register_peer_route(
    peer_routes: &PeerRouteMap,
    connection_id: u64,
    connection_tx: mpsc::Sender<SignalingMessage>,
    peer_id: &str,
    registered_peer_id: &mut Option<String>,
) {
    let mut routes = peer_routes.write().await;
    routes.insert(peer_id.to_string(), (connection_id, connection_tx));
    *registered_peer_id = Some(peer_id.to_string());
}

pub(super) async fn unregister_peer_route(
    peer_routes: &PeerRouteMap,
    connection_id: u64,
    peer_id: Option<&str>,
) {
    let Some(peer_id) = peer_id else {
        return;
    };

    let mut routes = peer_routes.write().await;
    if routes.get(peer_id).is_some_and(|(id, _)| *id == connection_id) {
        routes.remove(peer_id);
    }
}
