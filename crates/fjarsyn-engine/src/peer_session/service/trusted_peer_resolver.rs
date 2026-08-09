use async_trait::async_trait;

use crate::{
    identity::{PeerId, TrustedPeerIdentity},
    peer_session::Error,
};

#[async_trait]
pub(crate) trait TrustedPeerResolver: Send + Sync {
    async fn trusted_peer(&self, peer_id: &PeerId) -> Result<Option<TrustedPeerIdentity>, Error>;
}
