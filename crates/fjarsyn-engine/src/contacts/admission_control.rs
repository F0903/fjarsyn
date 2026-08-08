use async_trait::async_trait;

use crate::{
    identity::PeerId,
    peer_session::{self, TrustBarrierOwnerId},
};

#[async_trait]
pub(super) trait AdmissionControl: Send + Sync {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error>;

    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error>;
}

#[async_trait]
impl AdmissionControl for peer_session::ServiceHandle {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        peer_session::ServiceHandle::ensure_trust_suspended(self, peer_id, owner_id).await
    }

    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        peer_session::ServiceHandle::release_trust_suspension(self, peer_id, owner_id).await
    }
}
