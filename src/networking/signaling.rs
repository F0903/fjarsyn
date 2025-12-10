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
) -> Result<mpsc::Sender<SignalingMessage>> {
    let (ws_stream, _) =
        connect_async(SIGNALING_SERVER_URL).await.map_err(SignalingError::ConnectionFailed)?;

    tracing::info!("Successfully connected to signaling server");

    let (write, read) = ws_stream.split();

    // Channel for sending messages to the server's writer task
    let (to_server_tx, to_server_rx) = mpsc::channel::<SignalingMessage>(100);

    // Spawn a writer task
    spawn_writer_task(to_server_rx, write);

    // Spawn a reader task
    spawn_reader_task(read, to_webrtc_tx);

    Ok(to_server_tx)
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
    mut read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
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
