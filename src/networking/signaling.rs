use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use loki_shared::SignalingMessage;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};

use crate::networking::signaling_error::SignalingError;

type Result<T> = std::result::Result<T, SignalingError>;

const SIGNALING_SERVER_URL: &str = "ws://127.0.0.1:30000/ws";

/// Connects to the signaling server, returning a channel sender to send
/// messages to the server. Incoming messages from the server will be sent
/// to the `to_webrtc_tx` channel.
pub async fn connect(
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<(mpsc::Sender<SignalingMessage>, String)> {
    let (ws_stream, _) =
        connect_async(SIGNALING_SERVER_URL).await.map_err(SignalingError::ConnectionFailed)?;
    let (write, mut read) = ws_stream.split();

    tracing::info!("Successfully connected to signaling server. Waiting for ID response...");

    let id = match read
        .next()
        .await
        .ok_or(SignalingError::IdResponseError("No response".to_string()))??
    {
        Message::Text(body) => {
            let msg: SignalingMessage = serde_json::from_str(&body)
                .map_err(|e| SignalingError::IdResponseError(e.to_string()))?;
            msg.data
        }
        _ => return Err(SignalingError::IdResponseError("Invalid response content".to_string())),
    };

    tracing::info!("Got ID: {}", id);

    // Channel for sending messages to the server's writer task
    let (to_server_tx, to_server_rx) = mpsc::channel::<SignalingMessage>(100);
    spawn_writer_task(to_server_rx, write);
    spawn_reader_task(to_webrtc_tx, read);

    Ok((to_server_tx, id))
}

fn spawn_writer_task(
    mut to_server_rx: mpsc::Receiver<SignalingMessage>,
    mut write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) {
    tokio::spawn(async move {
        while let Some(message) = to_server_rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(json) => {
                    if write.send(Message::Text(json.into())).await.is_err() {
                        tracing::error!(
                            "Failed to send message to signaling server. WebSocket connection closed."
                        );
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to serialize signaling message: {}", e);
                }
            }
        }
        tracing::info!("Signaling WebSocket writer task finished.");
    });
}

fn spawn_reader_task(
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
    mut read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) {
    tokio::spawn(async move {
        while let Some(message) = read.next().await {
            match message {
                Ok(msg) => {
                    if let Message::Text(text) = msg {
                        match serde_json::from_str::<SignalingMessage>(&text) {
                            Ok(signaling_message) => {
                                if to_webrtc_tx.send(signaling_message).await.is_err() {
                                    tracing::error!(
                                        "Failed to send message to WebRTC task. Channel closed."
                                    );
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to deserialize signaling message: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Error receiving signaling message: {}", e);
                    break;
                }
            }
        }
        tracing::info!("Signaling WebSocket reader task finished.");
    });
}
