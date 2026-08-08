use chrono::{DateTime, Utc};

use super::{
    super::{Command, state_machine},
    Runtime,
    application_data::next_share_epoch,
};
use crate::peer_session::{
    CloseReason, Error, Event, LocalShareState, MessageId, ShareId,
    protocol::{ControlMessage, DATA_PROTOCOL_VERSION, MessagingMessage, NegotiationSignal},
};

impl Runtime {
    pub(super) async fn handle_command(&mut self, command: Command) {
        match command {
            Command::Accept(reply) => {
                let result = self.accept().await;
                if let Err(error) = &result
                    && !matches!(error, Error::InvalidState { .. })
                {
                    self.fail_error(error.clone());
                }
                let _ = reply.send(result);
            }
            Command::Reject { reason, reply } => {
                let result = self.reject(reason).await;
                let _ = reply.send(result);
            }
            Command::Disconnect(reply) => {
                let result = self.disconnect().await;
                let _ = reply.send(result);
            }
            Command::SendMessage { message_id, body, sent_at, reply } => {
                let result = self.send_message(message_id, body, sent_at).await;
                self.fail_on_message_command_error(&result);
                let _ = reply.send(result);
            }
            Command::SendReceipt { message_id, received_at, reply } => {
                let result = self.send_receipt(message_id, received_at).await;
                self.fail_on_message_command_error(&result);
                let _ = reply.send(result);
            }
            Command::StartShare(reply) => {
                let result = self.start_share().await;
                self.fail_on_terminal_command_error(&result);
                let _ = reply.send(result);
            }
            Command::StopShare { share_id, reply } => {
                let result = self.stop_share(share_id).await;
                self.fail_on_terminal_command_error(&result);
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Command::ForceIceRestart(reply) => {
                let result = self.begin_ice_restart_with_old_transport_recovery(false).await;
                if let Err(error) = &result {
                    self.fail_error(error.clone());
                }
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Command::CommittedTransportGeneration(reply) => {
                let _ = reply.send(Ok(self.restart.committed()));
            }
        }
    }

    async fn accept(&mut self) -> Result<(), Error> {
        self.ensure_rtc().await?;
        self.apply(state_machine::Input::AcceptLocal).await?;
        self.send_signal(NegotiationSignal::Accept {}).await
    }

    async fn reject(&mut self, reason: String) -> Result<(), Error> {
        if reason.len() > 512 {
            return Err(Error::Protocol("rejection reason exceeds limit".into()));
        }
        self.apply(state_machine::Input::RejectLocal(reason.clone())).await?;
        self.send_signal(NegotiationSignal::Reject { reason: reason.clone() }).await?;
        self.terminal_reason = Some(CloseReason::Rejected { reason });
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), Error> {
        self.apply(state_machine::Input::DisconnectLocal).await?;
        if self.application_data.control_open() {
            if let Some(rtc) = self.rtc.as_ref() {
                let payload = serde_json::to_string(&ControlMessage::Disconnect {
                    version: DATA_PROTOCOL_VERSION,
                })
                .map_err(|error| Error::Protocol(error.to_string()))?;
                let _ = rtc.send_control(payload).await;
            }
        } else {
            let _ = self.send_signal(NegotiationSignal::Cancel {}).await;
        }
        self.terminal_reason = Some(CloseReason::LocalDisconnect);
        Ok(())
    }

    async fn send_message(
        &mut self,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        self.require_connected("send a message")?;
        let message = MessagingMessage::Chat {
            version: DATA_PROTOCOL_VERSION,
            message_id,
            body: body.clone(),
            sent_at,
        };
        message.validate(self.config.max_message_bytes)?;
        let encoded =
            serde_json::to_string(&message).map_err(|error| Error::Protocol(error.to_string()))?;
        if encoded.len() > self.config.max_data_message_bytes {
            return Err(Error::MessageTooLarge { max: self.config.max_message_bytes });
        }
        self.rtc
            .as_ref()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
            .send_message(encoded)
            .await?;
        self.emit(Event::MessageSent {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            message_id,
            body,
            sent_at,
        })
        .await;
        Ok(())
    }

    async fn send_receipt(
        &mut self,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        self.require_connected("send a receipt")?;
        let message =
            MessagingMessage::Receipt { version: DATA_PROTOCOL_VERSION, message_id, received_at };
        let encoded =
            serde_json::to_string(&message).map_err(|error| Error::Protocol(error.to_string()))?;
        self.rtc
            .as_ref()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
            .send_message(encoded)
            .await
    }

    async fn start_share(&mut self) -> Result<ShareId, Error> {
        self.require_connected("start screen sharing")?;
        if !matches!(self.local_share, LocalShareState::Inactive) {
            return Err(self.invalid_state("start another screen share"));
        }
        let share_id = ShareId::new();
        let epoch = next_share_epoch(self.last_local_share_epoch)?;
        self.send_control(ControlMessage::ShareStarted {
            version: DATA_PROTOCOL_VERSION,
            share_id,
            epoch,
        })
        .await?;
        self.last_local_share_epoch = Some(epoch);
        self.local_share = LocalShareState::Active { share_id, epoch };
        self.active_video_tx.send_replace(Some((share_id, epoch)));
        self.publish_snapshot().await;
        self.emit(Event::LocalShareChanged {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            state: self.local_share,
        })
        .await;
        Ok(share_id)
    }

    async fn stop_share(&mut self, share_id: ShareId) -> Result<(), Error> {
        self.require_connected("stop screen sharing")?;
        let epoch = match self.local_share {
            LocalShareState::Active { share_id: active, epoch } if active == share_id => epoch,
            _ => return Err(Error::ShareMismatch(share_id)),
        };
        self.send_control(ControlMessage::ShareStopped {
            version: DATA_PROTOCOL_VERSION,
            share_id,
            epoch,
        })
        .await?;
        self.local_share = LocalShareState::Inactive;
        self.active_video_tx.send_replace(None);
        self.publish_snapshot().await;
        self.emit(Event::LocalShareChanged {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            state: self.local_share,
        })
        .await;
        Ok(())
    }

    async fn send_control(&self, message: ControlMessage) -> Result<(), Error> {
        let encoded =
            serde_json::to_string(&message).map_err(|error| Error::Protocol(error.to_string()))?;
        self.rtc
            .as_ref()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
            .send_control(encoded)
            .await
    }
}
