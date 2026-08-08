//! Data-channel readiness and bounded pre-ready application-data buffering.

use std::collections::VecDeque;

use bytes::Bytes;

use crate::peer_session::{
    Error, Phase,
    protocol::{ControlMessage, MessagingMessage},
    rtc::ChannelKind,
};

/// Owns data-channel readiness and the bounded window of frames that can race
/// the final signaling acknowledgement.
pub(super) struct ApplicationDataGate {
    peer_connected: bool,
    control_open: bool,
    messaging_open: bool,
    local_ready: bool,
    remote_ready: bool,
    ready_acknowledged: bool,
    pending: VecDeque<(ChannelKind, Bytes)>,
    capacity: usize,
    max_message_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Deliver,
    Buffer,
    Reject,
}

impl ApplicationDataGate {
    pub(super) fn new(capacity: usize, max_message_bytes: usize) -> Self {
        Self {
            peer_connected: false,
            control_open: false,
            messaging_open: false,
            local_ready: false,
            remote_ready: false,
            ready_acknowledged: false,
            pending: VecDeque::new(),
            capacity,
            max_message_bytes,
        }
    }

    pub(super) fn mark_peer_connected(&mut self) {
        self.peer_connected = true;
    }

    pub(super) fn mark_peer_disconnected(&mut self) {
        self.peer_connected = false;
    }

    pub(super) fn set_channel_open(&mut self, kind: ChannelKind, open: bool) {
        match kind {
            ChannelKind::Control => self.control_open = open,
            ChannelKind::Messaging => self.messaging_open = open,
        }
    }

    pub(super) fn transport_channels_open(&self) -> bool {
        self.control_open && self.messaging_open
    }

    pub(super) fn control_open(&self) -> bool {
        self.control_open
    }

    /// Starts a new transport-readiness handshake while preserving the data
    /// channels, which WebRTC keeps alive across an ICE restart.
    pub(super) fn reset_readiness(&mut self) {
        self.peer_connected = false;
        self.local_ready = false;
        self.remote_ready = false;
        self.ready_acknowledged = false;
    }

    pub(super) fn should_announce_ready(&self, phase: Phase) -> bool {
        matches!(phase, Phase::Negotiating | Phase::Reconnecting)
            && self.peer_connected
            && self.transport_channels_open()
            && !self.local_ready
    }

    pub(super) fn mark_local_ready(&mut self) {
        self.local_ready = true;
    }

    pub(super) fn accept_remote_ready(&mut self, phase: Phase) -> Result<(), Error> {
        if !matches!(phase, Phase::Negotiating | Phase::Reconnecting) || self.remote_ready {
            return Err(Error::Protocol("unexpected session-ready signal".into()));
        }
        self.remote_ready = true;
        Ok(())
    }

    pub(super) fn accept_ready_acknowledgement(&mut self, phase: Phase) -> Result<(), Error> {
        if !matches!(phase, Phase::Negotiating | Phase::Reconnecting)
            || !self.local_ready
            || self.ready_acknowledged
        {
            return Err(Error::Protocol("unexpected session-ready acknowledgement".into()));
        }
        self.ready_acknowledged = true;
        Ok(())
    }

    pub(super) fn handshake_complete(&self, phase: Phase) -> bool {
        matches!(phase, Phase::Negotiating | Phase::Reconnecting)
            && self.local_ready
            && self.remote_ready
            && self.ready_acknowledged
    }

    pub(super) fn mark_established(&mut self) {
        self.peer_connected = true;
        self.local_ready = true;
        self.remote_ready = true;
        self.ready_acknowledged = true;
    }

    /// Classifies and, when necessary, retains one incoming data-channel
    /// frame. `Some` means the actor may deliver it immediately; `None` means
    /// it was safely buffered by this gate.
    pub(super) fn route(
        &mut self,
        phase: Phase,
        kind: ChannelKind,
        data: Bytes,
    ) -> Result<Option<(ChannelKind, Bytes)>, Error> {
        let is_disconnect = if matches!(phase, Phase::Negotiating | Phase::Reconnecting)
            && kind == ChannelKind::Control
        {
            let message: ControlMessage = serde_json::from_slice(&data)
                .map_err(|error| Error::Protocol(error.to_string()))?;
            message.validate()?;
            matches!(message, ControlMessage::Disconnect { .. })
        } else {
            false
        };

        match self.disposition(phase, is_disconnect) {
            Disposition::Deliver => Ok(Some((kind, data))),
            Disposition::Buffer => {
                if self.pending.len() >= self.capacity {
                    return Err(Error::Protocol(
                        "too many application frames arrived before readiness".into(),
                    ));
                }
                validate_buffered(kind, &data, self.max_message_bytes)?;
                self.pending.push_back((kind, data));
                Ok(None)
            }
            Disposition::Reject => {
                Err(Error::Protocol("application data arrived outside an active session".into()))
            }
        }
    }

    pub(super) fn pop_pending(&mut self) -> Option<(ChannelKind, Bytes)> {
        self.pending.pop_front()
    }

    fn disposition(&self, phase: Phase, is_disconnect: bool) -> Disposition {
        match phase {
            Phase::Connected => Disposition::Deliver,
            Phase::Reconnecting if is_disconnect => Disposition::Deliver,
            Phase::Reconnecting => Disposition::Buffer,
            Phase::Negotiating if is_disconnect => Disposition::Deliver,
            Phase::Negotiating if self.local_ready && self.remote_ready => Disposition::Buffer,
            Phase::Negotiating => Disposition::Reject,
            Phase::Requesting | Phase::Incoming | Phase::Disconnecting => Disposition::Reject,
        }
    }
}

fn validate_buffered(
    kind: ChannelKind,
    data: &[u8],
    max_message_bytes: usize,
) -> Result<(), Error> {
    match kind {
        ChannelKind::Control => {
            let message: ControlMessage =
                serde_json::from_slice(data).map_err(|error| Error::Protocol(error.to_string()))?;
            message.validate()
        }
        ChannelKind::Messaging => {
            let message: MessagingMessage =
                serde_json::from_slice(data).map_err(|error| Error::Protocol(error.to_string()))?;
            message.validate(max_message_bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;
    use crate::peer_session::protocol::DATA_PROTOCOL_VERSION;

    #[test]
    fn disposition_matches_the_session_readiness_lifecycle() {
        let mut gate = ApplicationDataGate::new(2, 1024);
        assert_eq!(gate.disposition(Phase::Connected, false), Disposition::Deliver);
        assert_eq!(gate.disposition(Phase::Reconnecting, false), Disposition::Buffer);
        assert_eq!(gate.disposition(Phase::Reconnecting, true), Disposition::Deliver);
        for phase in [Phase::Requesting, Phase::Incoming, Phase::Disconnecting] {
            assert_eq!(gate.disposition(phase, false), Disposition::Reject);
        }
        assert_eq!(gate.disposition(Phase::Negotiating, false), Disposition::Reject);

        gate.mark_peer_connected();
        gate.set_channel_open(ChannelKind::Control, true);
        gate.set_channel_open(ChannelKind::Messaging, true);
        gate.mark_local_ready();
        assert_eq!(gate.disposition(Phase::Negotiating, false), Disposition::Reject);
        gate.accept_remote_ready(Phase::Negotiating).unwrap();
        assert_eq!(gate.disposition(Phase::Negotiating, false), Disposition::Buffer);
        assert_eq!(gate.disposition(Phase::Negotiating, true), Disposition::Deliver);
    }

    #[test]
    fn frames_racing_the_final_ready_ack_are_buffered_in_order() {
        let mut gate = ApplicationDataGate::new(2, 1024);
        gate.mark_local_ready();
        gate.accept_remote_ready(Phase::Negotiating).unwrap();

        let receipt = || {
            serde_json::to_vec(&MessagingMessage::Receipt {
                version: DATA_PROTOCOL_VERSION,
                message_id: crate::peer_session::MessageId::new(),
                received_at: chrono::Utc::now(),
            })
            .unwrap()
        };
        let first = receipt();
        let second = receipt();

        for data in [&first, &second] {
            assert!(
                gate.route(
                    Phase::Negotiating,
                    ChannelKind::Messaging,
                    Bytes::copy_from_slice(data),
                )
                .unwrap()
                .is_none()
            );
        }
        assert_eq!(gate.pop_pending().unwrap().1, Bytes::from(first));
        assert_eq!(gate.pop_pending().unwrap().1, Bytes::from(second));
        assert!(gate.pop_pending().is_none());
    }
}
