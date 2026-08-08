use std::{sync::Arc, time::Duration};

use webrtc::data_channel::{
    RTCDataChannel, data_channel_init::RTCDataChannelInit, data_channel_message::DataChannelMessage,
};

use super::{
    super::{ChannelKind, Event},
    Peer, rtc_operation,
};
use crate::peer_session::Error;

impl Peer {
    pub(in crate::peer_session) async fn prepare_offerer_channels(&mut self) -> Result<(), Error> {
        for kind in [ChannelKind::Control, ChannelKind::Messaging] {
            let options = RTCDataChannelInit {
                ordered: Some(true),
                protocol: Some(kind.label().to_owned()),
                ..Default::default()
            };
            let channel = rtc_operation(
                self.operation_timeout,
                self.pc.create_data_channel(kind.label(), Some(options)),
            )
            .await?;
            self.attach_data_channel(channel)?;
        }
        Ok(())
    }

    pub(in crate::peer_session) fn attach_data_channel(
        &mut self,
        channel: Arc<RTCDataChannel>,
    ) -> Result<(), Error> {
        let kind = ChannelKind::from_label(channel.label()).ok_or_else(|| {
            Error::Protocol(format!("unexpected data channel {}", channel.label()))
        })?;
        if !channel.ordered()
            || channel.max_packet_lifetime().is_some()
            || channel.max_retransmits().is_some()
            || channel.protocol() != kind.label()
        {
            return Err(Error::Protocol(format!(
                "data channel {} is not reliable and ordered",
                channel.label()
            )));
        }
        let slot = match kind {
            ChannelKind::Control => &mut self.control,
            ChannelKind::Messaging => &mut self.messaging,
        };
        if slot.is_some() {
            return Err(Error::Protocol(format!("duplicate data channel {}", channel.label())));
        }

        let open_events = self.events.clone();
        channel.on_open(Box::new(move || {
            let open_events = open_events.clone();
            Box::pin(async move {
                open_events.dispatch(Event::ChannelOpen(kind));
            })
        }));
        let close_events = self.events.clone();
        channel.on_close(Box::new(move || {
            let close_events = close_events.clone();
            Box::pin(async move {
                close_events.dispatch(Event::ChannelClosed(kind));
            })
        }));
        let message_events = self.events.clone();
        let max_data_message_bytes = self.max_data_message_bytes;
        channel.on_message(Box::new(move |message: DataChannelMessage| {
            let message_events = message_events.clone();
            Box::pin(async move {
                if let Err(reason) = validate_inbound_data_frame(
                    kind,
                    message.is_string,
                    &message.data,
                    max_data_message_bytes,
                ) {
                    message_events.dispatch(Event::ProtocolError(reason));
                } else {
                    message_events.dispatch(Event::ChannelMessage(kind, message.data));
                }
            })
        }));
        *slot = Some(channel);
        Ok(())
    }

    pub(in crate::peer_session) async fn send_control(&self, data: String) -> Result<(), Error> {
        send_data(self.control.as_ref(), data, "control", self.operation_timeout).await
    }

    pub(in crate::peer_session) async fn send_message(&self, data: String) -> Result<(), Error> {
        send_data(self.messaging.as_ref(), data, "messaging", self.operation_timeout).await
    }
}

async fn send_data(
    channel: Option<&Arc<RTCDataChannel>>,
    data: String,
    name: &str,
    timeout: Duration,
) -> Result<(), Error> {
    let channel =
        channel.ok_or_else(|| Error::WebRtc(format!("{name} data channel is unavailable")))?;
    tokio::time::timeout(timeout, channel.send_text(data))
        .await
        // SCTP may have accepted the frame before the future times out. The
        // transport is closed by the actor, but callers must not treat this as
        // a definite non-delivery result.
        .map_err(|_| Error::OutcomeUnknown)?
        .map(|_| ())
        .map_err(|error| Error::WebRtc(error.to_string()))
}

pub(super) fn validate_inbound_data_frame(
    kind: ChannelKind,
    is_string: bool,
    data: &[u8],
    max_bytes: usize,
) -> Result<(), String> {
    if !is_string {
        return Err(format!("{} data channel received an unexpected binary frame", kind.label()));
    }
    if std::str::from_utf8(data).is_err() {
        return Err(format!("{} data channel received invalid UTF-8", kind.label()));
    }
    if data.len() > max_bytes {
        return Err(format!(
            "{} data-channel frame exceeds the {} byte limit",
            kind.label(),
            max_bytes
        ));
    }
    Ok(())
}
