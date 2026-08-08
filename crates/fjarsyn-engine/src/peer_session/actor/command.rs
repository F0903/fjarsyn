use chrono::{DateTime, Utc};
use tokio::sync::oneshot;

#[cfg(test)]
use crate::peer_session::TransportGeneration;
use crate::peer_session::{Error, MessageId, ShareId};

#[derive(Debug)]
pub(in crate::peer_session) enum Command {
    Accept(oneshot::Sender<Result<(), Error>>),
    Reject {
        reason: String,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    Disconnect(oneshot::Sender<Result<(), Error>>),
    SendMessage {
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    SendReceipt {
        message_id: MessageId,
        received_at: DateTime<Utc>,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    StartShare(oneshot::Sender<Result<ShareId, Error>>),
    StopShare {
        share_id: ShareId,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    #[cfg(test)]
    ForceIceRestart(oneshot::Sender<Result<(), Error>>),
    #[cfg(test)]
    CommittedTransportGeneration(oneshot::Sender<Result<TransportGeneration, Error>>),
}

impl Command {
    pub(in crate::peer_session) fn reply_error(self, error: Error) {
        match self {
            Self::Accept(reply) | Self::Disconnect(reply) => {
                let _ = reply.send(Err(error));
            }
            Self::Reject { reply, .. }
            | Self::SendMessage { reply, .. }
            | Self::SendReceipt { reply, .. }
            | Self::StopShare { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::StartShare(reply) => {
                let _ = reply.send(Err(error));
            }
            #[cfg(test)]
            Self::ForceIceRestart(reply) => {
                let _ = reply.send(Err(error));
            }
            #[cfg(test)]
            Self::CommittedTransportGeneration(reply) => {
                let _ = reply.send(Err(error));
            }
        }
    }
}
