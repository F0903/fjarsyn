use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use super::{Error, Invite, MAX_INVITE_BYTES};
use crate::identity::{self, LocalIdentity, PeerId, PeerIdError};

const GOLDEN_PEER_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const GOLDEN_PUBLIC_KEY_BASE64: &str = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";
const GOLDEN_INVITE: &str = concat!(
    "fjarsyn:pair:v1:",
    "NTUwZTg0MDAtZTI5Yi00MWQ0LWE3MTYtNDQ2NjU1NDQwMDAw:",
    "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
);
const GOLDEN_FINGERPRINT: &str =
    concat!("6DB5 E7B5 7B70 8C85 16D0 D38C CEA8 8341 ", "D7B3 7EFE 1653 945B 806E B54D C7C1 C2C8");
const GOLDEN_PUBLIC_KEY_BYTES: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

fn golden_invite() -> Invite {
    Invite::new(PeerId::new(GOLDEN_PEER_ID).unwrap(), GOLDEN_PUBLIC_KEY_BASE64).unwrap()
}

#[test]
fn golden_invite_and_fingerprint_lock_the_wire_format() {
    let invite = golden_invite();

    assert_eq!(invite.to_string(), GOLDEN_INVITE);
    assert_eq!(invite.public_key, GOLDEN_PUBLIC_KEY_BYTES);
    assert_eq!(invite.fingerprint().to_string(), GOLDEN_FINGERPRINT);
    assert_eq!(GOLDEN_INVITE.parse::<Invite>().unwrap(), invite);
}

#[test]
fn parser_allows_only_outer_whitespace_and_display_is_canonical() {
    let parsed = format!(" \r\n{GOLDEN_INVITE}\t ").parse::<Invite>().unwrap();

    assert_eq!(parsed.to_string(), GOLDEN_INVITE);
    let peer_with_space = URL_SAFE_NO_PAD.encode(b"peer-a ");
    let token =
        format!("fjarsyn:pair:v1:{peer_with_space}:{}", URL_SAFE_NO_PAD.encode(parsed.public_key));
    assert!(matches!(
        token.parse::<Invite>(),
        Err(Error::InvalidPeerId(PeerIdError::InvalidCharacter { index: 6, character: ' ' }))
    ));
}

#[test]
fn parser_rejects_wrong_structure_and_version() {
    assert!(matches!("".parse::<Invite>(), Err(Error::InvalidFieldCount { actual: 1, .. })));
    assert!(matches!(
        format!("{GOLDEN_INVITE}:extra").parse::<Invite>(),
        Err(Error::InvalidFieldCount { actual: 6, .. })
    ));
    assert!(matches!(
        GOLDEN_INVITE.replacen("fjarsyn", "other", 1).parse::<Invite>(),
        Err(Error::InvalidScheme(_))
    ));
    assert!(matches!(
        GOLDEN_INVITE.replacen(":pair:", ":other:", 1).parse::<Invite>(),
        Err(Error::InvalidKind(_))
    ));
    assert!(matches!(
        GOLDEN_INVITE.replacen(":v1:", ":v2:", 1).parse::<Invite>(),
        Err(Error::UnsupportedVersion(version)) if version == "v2"
    ));
}

#[test]
fn parser_rejects_noncanonical_and_malformed_fields() {
    let padded = format!("{GOLDEN_INVITE}=");
    assert!(matches!(
        padded.parse::<Invite>(),
        Err(Error::InvalidBase64Url { field: "public key", .. })
            | Err(Error::NonCanonicalBase64Url { field: "public key" })
    ));

    let invalid_utf8 =
        format!("fjarsyn:pair:v1:_w:{}", URL_SAFE_NO_PAD.encode(golden_invite().public_key));
    assert!(matches!(invalid_utf8.parse::<Invite>(), Err(Error::InvalidPeerIdUtf8(_))));

    let short_key = URL_SAFE_NO_PAD.encode([7_u8; 31]);
    let short = format!("fjarsyn:pair:v1:cGVlcg:{short_key}");
    assert!(matches!(
        short.parse::<Invite>(),
        Err(Error::InvalidPublicKeyLength { actual: 31, .. })
    ));
}

#[test]
fn parser_rejects_weak_ed25519_keys() {
    let mut identity_point = [0_u8; 32];
    identity_point[0] = 1;
    let weak = format!("fjarsyn:pair:v1:cGVlcg:{}", URL_SAFE_NO_PAD.encode(identity_point));

    assert!(matches!(
        weak.parse::<Invite>(),
        Err(Error::InvalidIdentity(identity::Error::WeakPublicKey))
    ));
}

#[test]
fn parser_caps_the_complete_untrusted_input() {
    let oversized = format!("{}{}", " ".repeat(MAX_INVITE_BYTES), GOLDEN_INVITE);

    assert!(matches!(oversized.parse::<Invite>(), Err(Error::TooLong { max: MAX_INVITE_BYTES })));
}

#[test]
fn any_identity_mutation_changes_the_full_fingerprint() {
    let invite = golden_invite();
    let changed_peer = Invite::new(
        PeerId::new("550e8400-e29b-41d4-a716-446655440001").unwrap(),
        GOLDEN_PUBLIC_KEY_BASE64,
    )
    .unwrap();
    let changed_key = Invite::from_local(&LocalIdentity::generate(invite.peer_id().clone()));

    assert_ne!(invite.fingerprint(), changed_peer.fingerprint());
    assert_ne!(invite.fingerprint(), changed_key.fingerprint());
}

#[test]
fn confirmation_preserves_the_exact_validated_identity() {
    let invite = golden_invite();
    let expected_fingerprint = invite.fingerprint();
    let expected_identity = invite.trusted_identity();
    let verified = invite.confirm();

    assert_eq!(verified.peer_id(), &expected_identity.peer_id);
    assert_eq!(verified.public_key_base64(), expected_identity.public_key);
    assert_eq!(verified.fingerprint(), expected_fingerprint);
    assert_eq!(verified.trusted_identity(), &expected_identity);
    assert_eq!(verified.into_trusted_identity(), expected_identity);
}

#[test]
fn local_invites_round_trip_through_the_text_format() {
    let peer_id = PeerId::new("local-peer_1.example").unwrap();
    let invite = Invite::from_local(&LocalIdentity::generate(peer_id));

    assert_eq!(invite.to_string().parse::<Invite>().unwrap(), invite);
}
