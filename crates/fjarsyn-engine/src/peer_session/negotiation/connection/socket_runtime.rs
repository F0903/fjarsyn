use chrono::Utc;
use futures::{SinkExt, StreamExt};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Error as WebSocketError, protocol::Message},
};

use super::{SessionConnectionContext, handshake::parse_envelope};
use crate::peer_session::{
    Error,
    negotiation::Limits,
    protocol::{
        EnvelopeVerification, NegotiationSignal, SessionReplayCache, SignedSessionEnvelope,
    },
};

pub(super) struct SocketRuntime {
    outbound_tx: mpsc::Sender<OutboundEnvelope>,
    inbound_rx: mpsc::Receiver<Result<NegotiationSignal, Error>>,
    shutdown_tx: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl SocketRuntime {
    pub(super) fn spawn<S>(
        socket: WebSocketStream<S>,
        context: SessionConnectionContext,
        mut replay: SessionReplayCache,
    ) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let SessionConnectionContext {
            session_id,
            local_peer_id,
            remote_peer_id,
            local_identity: _,
            trusted_peer,
            limits,
        } = context;
        let (mut writer, mut reader) = socket.split();
        let (outbound_tx, mut outbound_rx) =
            mpsc::channel::<OutboundEnvelope>(limits.queue_capacity.max(1));
        let (inbound_tx, inbound_rx) = mpsc::channel(limits.queue_capacity.max(1));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            let _ = writer.send(Message::Close(None)).await;
                            break;
                        }
                    }
                    outbound = outbound_rx.recv() => {
                        let Some(outbound) = outbound else {
                            let _ = writer.send(Message::Close(None)).await;
                            break;
                        };
                        if !write_outbound(&mut writer, outbound, &limits, &inbound_tx).await {
                            break;
                        }
                    }
                    message = tokio::time::timeout(limits.idle_timeout, reader.next()) => {
                        let envelope = match receive_inbound(message, &limits, &inbound_tx) {
                            Some(envelope) => envelope,
                            None => break,
                        };
                        if let Err(error) = envelope.verify(
                            EnvelopeVerification {
                                trusted_peer: &trusted_peer,
                                expected_local: &local_peer_id,
                                expected_remote: Some(&remote_peer_id),
                                expected_session: Some(session_id),
                                now: Utc::now(),
                                max_age: limits.max_message_age,
                                max_clock_skew: limits.max_clock_skew,
                            },
                            &mut replay,
                        ) {
                            let _ = inbound_tx.try_send(Err(error));
                            break;
                        }
                        match tokio::time::timeout(
                            limits.handshake_timeout,
                            inbound_tx.send(Ok(envelope.into_payload())),
                        ).await {
                            Ok(Ok(())) => {}
                            _ => break,
                        }
                    }
                }
            }
        });

        Self { outbound_tx, inbound_rx, shutdown_tx, task: Some(task) }
    }

    pub(super) async fn send(&self, envelope: SignedSessionEnvelope) -> Result<(), Error> {
        let (written_tx, written_rx) = oneshot::channel();
        self.outbound_tx
            .send(OutboundEnvelope { envelope, written: written_tx })
            .await
            .map_err(|_| Error::Signaling("signaling connection closed".into()))?;
        written_rx.await.map_err(|_| Error::Signaling("signaling writer stopped".into()))?
    }

    pub(super) async fn recv(&mut self) -> Option<Result<NegotiationSignal, Error>> {
        self.inbound_rx.recv().await
    }

    pub(super) async fn shutdown_until(&mut self, deadline: tokio::time::Instant) {
        let _ = self.shutdown_tx.send(true);
        let Some(task) = self.task.as_mut() else {
            return;
        };
        if tokio::time::timeout_at(deadline, &mut *task).await.is_err() {
            task.abort();
        }
        self.task.take();
    }
}

impl Drop for SocketRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

struct OutboundEnvelope {
    envelope: SignedSessionEnvelope,
    written: oneshot::Sender<Result<(), Error>>,
}

async fn write_outbound<S>(
    writer: &mut futures::stream::SplitSink<WebSocketStream<S>, Message>,
    outbound: OutboundEnvelope,
    limits: &Limits,
    inbound_tx: &mpsc::Sender<Result<NegotiationSignal, Error>>,
) -> bool
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let serialized = match serde_json::to_string(&outbound.envelope) {
        Ok(serialized) => serialized,
        Err(error) => {
            let error = Error::Protocol(error.to_string());
            let _ = outbound.written.send(Err(error.clone()));
            let _ = inbound_tx.try_send(Err(error));
            return false;
        }
    };
    match tokio::time::timeout(
        limits.handshake_timeout,
        writer.send(Message::Text(serialized.into())),
    )
    .await
    {
        Ok(Ok(())) => {
            let _ = outbound.written.send(Ok(()));
            true
        }
        Ok(Err(error)) => {
            let error = Error::Signaling(error.to_string());
            let _ = outbound.written.send(Err(error.clone()));
            let _ = inbound_tx.try_send(Err(error));
            false
        }
        Err(_) => {
            let error = Error::Signaling("signaling write timed out".into());
            let _ = outbound.written.send(Err(error.clone()));
            let _ = inbound_tx.try_send(Err(error));
            false
        }
    }
}

fn receive_inbound(
    message: Result<Option<Result<Message, WebSocketError>>, tokio::time::error::Elapsed>,
    limits: &Limits,
    inbound_tx: &mpsc::Sender<Result<NegotiationSignal, Error>>,
) -> Option<SignedSessionEnvelope> {
    match message {
        Err(_) => {
            let _ = inbound_tx
                .try_send(Err(Error::Signaling("signaling connection idle timeout".into())));
            None
        }
        Ok(None) => None,
        Ok(Some(Err(error))) => {
            if !matches!(error, WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) {
                let _ = inbound_tx.try_send(Err(Error::Signaling(error.to_string())));
            }
            None
        }
        Ok(Some(Ok(message))) => match parse_envelope(message, limits.max_frame_bytes) {
            Ok(envelope) => Some(envelope),
            Err(error) => {
                let _ = inbound_tx.try_send(Err(error));
                None
            }
        },
    }
}
