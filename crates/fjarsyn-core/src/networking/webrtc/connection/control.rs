use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use webrtc::{
    data_channel::{
        RTCDataChannel, data_channel_message::DataChannelMessage,
        data_channel_state::RTCDataChannelState,
    },
    peer_connection::RTCPeerConnection,
};

use super::{WebRTC, WebRTCEvent};
use crate::networking::webrtc::{WebRTCError, webrtc_error::WebRTCResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlMessage {
    StreamStarted,
    StreamEnded,
}

impl ControlMessage {
    fn as_str(self) -> &'static str {
        match self {
            Self::StreamStarted => "stream-started",
            Self::StreamEnded => "stream-ended",
        }
    }

    fn from_message(message: &DataChannelMessage) -> Option<Self> {
        if !message.is_string {
            return None;
        }

        let payload = std::str::from_utf8(&message.data).ok()?;

        match payload {
            "stream-started" => Some(Self::StreamStarted),
            "stream-ended" => Some(Self::StreamEnded),
            _ => None,
        }
    }
}

impl WebRTC {
    pub(super) const CONTROL_CHANNEL_LABEL: &str = "fjarsyn-control";

    pub(super) fn setup_control_channel_handler(
        pc: &Arc<RTCPeerConnection>,
        control_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>>,
        event_tx: mpsc::Sender<WebRTCEvent>,
    ) {
        pc.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let control_channel = control_channel.clone();
            let event_tx = event_tx.clone();

            Box::pin(async move {
                if channel.label() != Self::CONTROL_CHANNEL_LABEL {
                    return;
                }

                tracing::debug!("Attached remote control channel: {}", channel.label());
                Self::register_control_channel(channel, control_channel, event_tx).await;
            })
        }));
    }

    pub(super) async fn ensure_control_channel(
        &self,
        pc: &Arc<RTCPeerConnection>,
    ) -> WebRTCResult<()> {
        if self.control_channel.read().await.is_some() {
            return Ok(());
        }

        let channel = pc
            .create_data_channel(Self::CONTROL_CHANNEL_LABEL, None)
            .await
            .map_err(WebRTCError::PeerConnectionError)?;

        tracing::debug!("Created local control channel: {}", channel.label());
        Self::register_control_channel(
            channel,
            self.control_channel.clone(),
            self.event_tx.clone(),
        )
        .await;

        Ok(())
    }

    pub(super) async fn clear_control_channel(&self) {
        *self.control_channel.write().await = None;
    }

    pub async fn notify_stream_started(&self) -> WebRTCResult<()> {
        self.send_control_message(ControlMessage::StreamStarted).await
    }

    pub async fn notify_stream_ended(&self) -> WebRTCResult<()> {
        self.send_control_message(ControlMessage::StreamEnded).await
    }

    async fn register_control_channel(
        channel: Arc<RTCDataChannel>,
        control_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>>,
        event_tx: mpsc::Sender<WebRTCEvent>,
    ) {
        *control_channel.write().await = Some(channel.clone());

        channel.on_message(Box::new(move |message: DataChannelMessage| {
            let event_tx = event_tx.clone();

            Box::pin(async move {
                let Some(control_message) = ControlMessage::from_message(&message) else {
                    tracing::debug!("Ignoring unknown control message: {:?}", message);
                    return;
                };

                let event = match control_message {
                    ControlMessage::StreamStarted => WebRTCEvent::RemoteStreamStarted,
                    ControlMessage::StreamEnded => WebRTCEvent::RemoteStreamEnded,
                };

                let _ = event_tx.send(event).await;
            })
        }));
    }

    async fn send_control_message(&self, message: ControlMessage) -> WebRTCResult<()> {
        let channel = self.control_channel.read().await.clone();
        let Some(channel) = channel else {
            tracing::debug!(
                "Skipping control message {} because no control channel is available.",
                message.as_str()
            );
            return Ok(());
        };

        if channel.ready_state() != RTCDataChannelState::Open {
            tracing::debug!(
                "Skipping control message {} because channel is {}.",
                message.as_str(),
                channel.ready_state()
            );
            return Ok(());
        }

        channel
            .send_text(message.as_str())
            .await
            .map(|_| ())
            .map_err(WebRTCError::PeerConnectionError)
    }
}
