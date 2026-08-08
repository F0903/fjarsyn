use chrono::{DateTime, Utc};

use super::{super::state_machine, Runtime};
use crate::peer_session::{
    CloseReason, Error, Event, Phase, RemoteShareState, ShareEpoch,
    protocol::{ControlMessage, MessagingMessage},
    rtc::ChannelKind,
};

pub(super) fn next_share_epoch(previous: Option<ShareEpoch>) -> Result<ShareEpoch, Error> {
    match previous {
        Some(previous) => previous.next(),
        None => Ok(ShareEpoch::FIRST),
    }
}

fn require_next_share_epoch(
    previous: Option<ShareEpoch>,
    received: ShareEpoch,
) -> Result<(), Error> {
    received.require_valid()?;
    let expected = next_share_epoch(previous)?;
    if received != expected {
        return Err(Error::Protocol(format!(
            "remote screen-share epoch {} did not use expected epoch {}",
            received.value(),
            expected.value()
        )));
    }
    Ok(())
}

impl Runtime {
    pub(super) async fn route_channel_message(
        &mut self,
        kind: ChannelKind,
        data: bytes::Bytes,
    ) -> Result<(), Error> {
        if let Some((kind, data)) = self.application_data.route(self.state.phase(), kind, data)? {
            self.handle_channel_message(kind, data).await?;
        }
        Ok(())
    }

    pub(super) async fn flush_pending_application_data(&mut self) -> Result<(), Error> {
        while let Some((kind, data)) = self.application_data.pop_pending() {
            self.handle_channel_message(kind, data).await?;
        }
        Ok(())
    }

    async fn handle_channel_message(
        &mut self,
        kind: ChannelKind,
        data: bytes::Bytes,
    ) -> Result<(), Error> {
        if data.len() > self.config.max_data_message_bytes {
            return Err(Error::Protocol("data-channel message exceeds limit".into()));
        }
        match kind {
            ChannelKind::Control => {
                let message: ControlMessage = serde_json::from_slice(&data)
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                message.validate()?;
                match message {
                    ControlMessage::ShareStarted { share_id, epoch, .. } => {
                        require_connected_application_data(self.state.phase())?;
                        if !matches!(self.remote_share, RemoteShareState::Inactive) {
                            return Err(Error::Protocol(
                                "remote started a second screen share".into(),
                            ));
                        }
                        require_next_share_epoch(self.last_remote_share_epoch, epoch)?;
                        self.last_remote_share_epoch = Some(epoch);
                        self.remote_share = RemoteShareState::Active { share_id, epoch };
                        self.publish_snapshot().await;
                        self.emit(Event::RemoteShareChanged {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            state: self.remote_share,
                        })
                        .await;
                    }
                    ControlMessage::ShareStopped { share_id, epoch, .. } => {
                        require_connected_application_data(self.state.phase())?;
                        match self.remote_share {
                            RemoteShareState::Active {
                                share_id: active_share_id,
                                epoch: active_epoch,
                            } if active_share_id == share_id && active_epoch == epoch => {}
                            RemoteShareState::Active { share_id: active_share_id, .. }
                                if active_share_id == share_id =>
                            {
                                return Err(Error::Protocol(
                                    "remote stopped the active screen share with the wrong epoch"
                                        .into(),
                                ));
                            }
                            RemoteShareState::Active { .. } | RemoteShareState::Inactive => {
                                return Err(Error::ShareMismatch(share_id));
                            }
                        }
                        self.remote_share = RemoteShareState::Inactive;
                        self.publish_snapshot().await;
                        self.emit(Event::RemoteShareChanged {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            state: self.remote_share,
                        })
                        .await;
                    }
                    ControlMessage::Disconnect { .. } => {
                        let _ = self.apply(state_machine::Input::DisconnectRemote).await;
                        self.terminal_reason = Some(CloseReason::RemoteDisconnect);
                    }
                }
            }
            ChannelKind::Messaging => {
                require_connected_application_data(self.state.phase())?;
                let message: MessagingMessage = serde_json::from_slice(&data)
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                message.validate(self.config.max_message_bytes)?;
                match message {
                    MessagingMessage::Chat { message_id, body, sent_at, .. } => {
                        validate_remote_timestamp(
                            sent_at,
                            Utc::now(),
                            self.config.max_remote_timestamp_age,
                            self.config.max_remote_clock_skew,
                        )?;
                        self.emit(Event::MessageReceived {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            message_id,
                            body,
                            sent_at,
                        })
                        .await;
                    }
                    MessagingMessage::Receipt { message_id, received_at, .. } => {
                        validate_remote_timestamp(
                            received_at,
                            Utc::now(),
                            self.config.max_remote_timestamp_age,
                            self.config.max_remote_clock_skew,
                        )?;
                        self.emit(Event::MessageReceiptReceived {
                            session_id: self.config.session_id,
                            peer_id: self.config.remote_peer_id.clone(),
                            message_id,
                            received_at,
                        })
                        .await;
                    }
                }
            }
        }
        Ok(())
    }
}

fn require_connected_application_data(phase: Phase) -> Result<(), Error> {
    if matches!(phase, Phase::Connected | Phase::Reconnecting) {
        Ok(())
    } else {
        Err(Error::Protocol("application data arrived before session readiness".into()))
    }
}

fn validate_remote_timestamp(
    timestamp: DateTime<Utc>,
    now: DateTime<Utc>,
    max_age: std::time::Duration,
    max_clock_skew: std::time::Duration,
) -> Result<(), Error> {
    let max_age = chrono::Duration::from_std(max_age)
        .map_err(|_| Error::Protocol("invalid remote timestamp age limit".into()))?;
    let max_clock_skew = chrono::Duration::from_std(max_clock_skew)
        .map_err(|_| Error::Protocol("invalid remote clock skew limit".into()))?;
    if timestamp < now - max_age || timestamp > now + max_clock_skew {
        return Err(Error::Protocol(
            "remote message timestamp is outside the accepted window".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_share_epochs_are_nonzero_and_exactly_monotonic() {
        let first = ShareEpoch::FIRST;
        let second = first.next().unwrap();

        assert!(require_next_share_epoch(None, first).is_ok());
        assert!(require_next_share_epoch(None, ShareEpoch::from_value(0)).is_err());
        assert!(require_next_share_epoch(Some(first), first).is_err());
        assert!(require_next_share_epoch(Some(first), second).is_ok());
        assert!(
            require_next_share_epoch(Some(first), ShareEpoch::from_value(second.value() + 1))
                .is_err()
        );
    }

    #[test]
    fn remote_timestamps_are_bounded() {
        let now = Utc::now();
        let age = std::time::Duration::from_secs(300);
        let skew = std::time::Duration::from_secs(30);

        assert!(validate_remote_timestamp(now, now, age, skew).is_ok());
        assert!(
            validate_remote_timestamp(now - chrono::Duration::hours(1), now, age, skew).is_err()
        );
        assert!(
            validate_remote_timestamp(now + chrono::Duration::hours(1), now, age, skew).is_err()
        );
    }
}
