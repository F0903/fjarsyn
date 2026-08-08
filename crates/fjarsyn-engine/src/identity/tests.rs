use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use super::{
    Error, LocalPeerIdentity, PeerId, TrustedPeerIdentity, key_encoding::PUBLIC_KEY_BYTES,
};

#[test]
fn generated_identity_round_trips_without_exposing_private_material_in_debug() {
    let identity = LocalPeerIdentity::generate();
    let stored = identity.to_stored();
    let restored = LocalPeerIdentity::from_stored(&stored).unwrap();

    assert_eq!(restored.public_key_base64(), identity.public_key_base64());
    assert!(!format!("{stored:?}").contains(&stored.private_key));
}

#[test]
fn mismatched_stored_keypair_is_rejected() {
    let mut stored = LocalPeerIdentity::generate().to_stored();
    stored.public_key = LocalPeerIdentity::generate().public_key_base64();

    assert!(matches!(LocalPeerIdentity::from_stored(&stored), Err(Error::PublicKeyMismatch)));
}

#[test]
fn trusted_identity_validates_key_encoding() {
    assert!(
        TrustedPeerIdentity::new(PeerId::new("peer-a").unwrap(), "not-a-key").validate().is_err()
    );
}

#[test]
fn trusted_identity_rejects_weak_ed25519_public_keys() {
    let mut identity_point = [0_u8; PUBLIC_KEY_BYTES];
    identity_point[0] = 1;
    let identity =
        TrustedPeerIdentity::new(PeerId::new("peer-a").unwrap(), BASE64.encode(identity_point));

    assert!(matches!(identity.validate(), Err(Error::WeakPublicKey)));
}
