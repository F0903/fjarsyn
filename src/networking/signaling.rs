use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{broadcast, mpsc},
};
use tokio_tungstenite::{
    WebSocketStream, accept_async, connect_async, tungstenite::protocol::Message,
};

use crate::networking::{protocol::SignalingMessage, signaling_error::SignalingError};

type Result<T> = std::result::Result<T, SignalingError>;

/// Connects directly to a peer's signaling listener.
pub async fn dial(
    addr: SocketAddr,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<mpsc::Sender<SignalingMessage>> {
    let url = format!("ws://{}/ws", addr);
    tracing::debug!("Connecting to signaling URL: {}", url);

    let (ws_stream, _) = connect_async(url).await.map_err(|e| {
        tracing::error!("Failed to connect to signaling at {}: {}", addr, e);
        SignalingError::ConnectionFailed(e)
    })?;

    let (to_peer_tx, to_peer_rx) = mpsc::channel::<SignalingMessage>(100);

    tokio::spawn(manage_dialer_connection(ws_stream, to_peer_rx, to_webrtc_tx));

    Ok(to_peer_tx)
}

/// Starts a signaling listener on the specified port.
pub async fn listen(
    port: u16,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<(mpsc::Sender<SignalingMessage>, u16)> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        tracing::error!("Failed to bind signaling listener to {}: {}", addr, e);
        SignalingError::ConnectionFailed(e.into())
    })?;

    let bound_port = listener.local_addr().unwrap().port();

    let (to_peer_tx, mut to_peer_rx_source) = mpsc::channel::<SignalingMessage>(100);
    let (broadcast_tx, _) = broadcast::channel::<SignalingMessage>(100);
    let broadcast_tx_clone = broadcast_tx.clone();

    // Forward mpsc to broadcast for many-to-many direct signaling
    tokio::spawn(async move {
        while let Some(msg) = to_peer_rx_source.recv().await {
            let _ = broadcast_tx_clone.send(msg);
        }
    });

    tokio::spawn(async move {
        tracing::info!("P2P Signaling listener active on 0.0.0.0:{}", bound_port);
        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let to_webrtc_tx = to_webrtc_tx.clone();
                    let to_peer_rx = broadcast_tx.subscribe();

                    tokio::spawn(async move {
                        tracing::info!("New signaling connection from {}", peer_addr);
                        if let Ok(ws_stream) = accept_async(stream).await {
                            manage_listener_connection(ws_stream, to_peer_rx, to_webrtc_tx).await;
                        } else {
                            tracing::error!("Failed to accept WebSocket from {}", peer_addr);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("TCP accept error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok((to_peer_tx, bound_port))
}

async fn manage_dialer_connection<S>(
    mut ws_stream: WebSocketStream<S>,
    mut to_peer_rx: mpsc::Receiver<SignalingMessage>,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            msg = to_peer_rx.recv() => {
                match msg {
                    Some(message) => {
                        if send_signaling_message(&mut ws_stream, &message).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("Signaling channel dropped. Closing WebSocket dialer.");
                        let _ = ws_stream.close(None).await;
                        break;
                    }
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(signaling_message) =
                            serde_json::from_str::<SignalingMessage>(&text)
                            && to_webrtc_tx.send(signaling_message).await.is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("WebSocket dialer closed by remote.");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("WebSocket dialer read error: {}", e);
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }
        }
    }
}

async fn manage_listener_connection<S>(
    mut ws_stream: WebSocketStream<S>,
    mut to_peer_rx: broadcast::Receiver<SignalingMessage>,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            msg = to_peer_rx.recv() => {
                match msg {
                    Ok(message) => {
                        if send_signaling_message(&mut ws_stream, &message).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Broadcast channel closed. Closing WebSocket listener.");
                        let _ = ws_stream.close(None).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::warn!("Signaling listener lagged behind broadcast channel.");
                    }
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(signaling_message) =
                            serde_json::from_str::<SignalingMessage>(&text)
                            && to_webrtc_tx.send(signaling_message).await.is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("WebSocket listener closed by remote.");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("WebSocket listener read error: {}", e);
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }
        }
    }
}

async fn send_signaling_message<S>(
    ws_stream: &mut WebSocketStream<S>,
    msg: &SignalingMessage,
) -> std::result::Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match serde_json::to_string(msg) {
        Ok(json) => {
            if ws_stream.send(Message::Text(json.into())).await.is_err() {
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            tracing::error!("Failed to serialize signaling message: {}", e);
            Err(())
        }
    }
}
