use tokio::sync::oneshot;

use super::TrustBarrierOwnerId;
use crate::{
    identity::PeerId,
    peer_session::{EncodedVideoSink, Error, RemoteVideoSource, SessionId, ShareId, actor},
};

#[derive(Debug)]
pub(in crate::peer_session::service) enum Command {
    Connect {
        peer_id: PeerId,
        reply: oneshot::Sender<Result<SessionId, Error>>,
    },
    EnsureTrustSuspended {
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    ReleaseTrustSuspension {
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    Session {
        session_id: SessionId,
        command: actor::Command,
    },
    EncodedVideoSink {
        session_id: SessionId,
        share_id: ShareId,
        reply: oneshot::Sender<Result<EncodedVideoSink, Error>>,
    },
    RemoteVideoSource {
        session_id: SessionId,
        reply: oneshot::Sender<Result<RemoteVideoSource, Error>>,
    },
}
