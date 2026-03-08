use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
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
        SignalingError::ConnectionFailed(e.into())
    })?;
    let (write, read) = ws_stream.split();

    let (to_peer_tx, to_peer_rx) = mpsc::channel::<SignalingMessage>(100);

    spawn_writer_task(to_peer_rx, write);
    spawn_reader_task(to_webrtc_tx, read);

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
                    tokio::spawn(handle_incoming_connection(
                        stream,
                        peer_addr,
                        to_webrtc_tx,
                        to_peer_rx,
                    ));
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

async fn handle_incoming_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
    to_peer_rx: broadcast::Receiver<SignalingMessage>,
) {
    tracing::info!("New signaling connection from {}", peer_addr);

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("Failed to accept WebSocket from {}: {}", peer_addr, e);
            return;
        }
    };

    let (write, read) = ws_stream.split();
    spawn_reader_task(to_webrtc_tx, read);
    spawn_broadcast_writer_task(to_peer_rx, write);
}

fn spawn_writer_task<S>(
    mut to_peer_rx: mpsc::Receiver<SignalingMessage>,
    write: SplitSink<WebSocketStream<S>, Message>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut write = write;
        while let Some(message) = to_peer_rx.recv().await {
            if send_signaling_message(&mut write, &message).await.is_err() {
                break;
            }
        }
    });
}

fn spawn_broadcast_writer_task<S>(
    mut to_peer_rx: broadcast::Receiver<SignalingMessage>,
    write: SplitSink<WebSocketStream<S>, Message>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut write = write;
        while let Ok(message) = to_peer_rx.recv().await {
            if send_signaling_message(&mut write, &message).await.is_err() {
                break;
            }
        }
    });
}

async fn send_signaling_message<S>(
    write: &mut SplitSink<WebSocketStream<S>, Message>,
    msg: &SignalingMessage,
) -> std::result::Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match serde_json::to_string(msg) {
        Ok(json) => {
            if write.send(Message::Text(json.into())).await.is_err() {
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

fn spawn_reader_task<S>(
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
    mut read: futures_util::stream::SplitStream<WebSocketStream<S>>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(signaling_message) = serde_json::from_str::<SignalingMessage>(&text) {
                        let _ = to_webrtc_tx.send(signaling_message).await;
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::error!("WebSocket read error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });
}
