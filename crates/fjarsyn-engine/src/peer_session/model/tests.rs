use std::sync::Arc;

use super::*;
use crate::identity::PeerId;

#[test]
fn peer_ids_validate_before_entering_the_peer_session_api() {
    assert_eq!(PeerId::new("peer-a").unwrap().as_str(), "peer-a");
    assert!(PeerId::new(" peer-a ").is_err());
}

#[test]
fn snapshot_lookups_use_typed_identifiers() {
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer-a").unwrap();
    let snapshot = Sessions {
        sessions: Arc::new(vec![SessionState {
            session_id,
            peer_id: peer_id.clone(),
            phase: Phase::Connected,
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        }]),
    };

    assert_eq!(snapshot.session(session_id).unwrap().peer_id, peer_id);
    assert_eq!(snapshot.session_for_peer(&peer_id).unwrap().session_id, session_id);
}

#[test]
fn share_epoch_is_nonzero_and_never_wraps() {
    assert!(ShareEpoch::try_from(0).is_err());
    assert_eq!(ShareEpoch::try_from(1).unwrap(), ShareEpoch::FIRST);
    assert!(ShareEpoch::try_from(u64::MAX).unwrap().next().is_err());
}
