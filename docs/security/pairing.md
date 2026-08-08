# Peer pairing and fingerprint verification

Fjarsyn pairs peers without a server, account directory or QR-code requirement.
The canonical Rust value is `pairing::Invite`; its transport representation is
a copyable, versioned text invite:

```text
fjarsyn:pair:v1:<peer-id-base64url>:<ed25519-public-key-base64url>
```

The two encoded fields use unpadded
[RFC 4648 base64url](https://www.rfc-editor.org/rfc/rfc4648.html#section-5).
The public-key field is the 32-byte Ed25519 public key defined by
[RFC 8032](https://www.rfc-editor.org/rfc/rfc8032.html#section-5.1.5). The
invite contains public identity material only; it is not a password or bearer
secret.

The same public key is encoded as an
[RFC 7250](https://www.rfc-editor.org/rfc/rfc7250) SubjectPublicKeyInfo value
and pinned as the contact's TLS 1.3 signaling raw public key. Fjarsyn does not
use hostnames, public certificate authorities or mDNS data as trust anchors.

## Verification ceremony

1. Each peer copies their own pairing invite and sends it to the other peer.
2. Each peer pastes the received invite, chooses a local contact name and
   views the derived PeerId and full fingerprint.
3. They compare the entire fingerprint in person or over a separate trusted
   channel, such as an already authenticated voice conversation.
4. Each peer explicitly confirms the comparison before Fjarsyn persists the
   trusted identity.

Pairing is reciprocal because incoming signaling is accepted only from an
identity already trusted by the receiver. Both peers therefore import and
verify each other's invite before either initiates a session.

Sending the invite and its fingerprint through the same potentially
compromised channel is not independent verification. The ceremony proves that
the saved key is the one controlled by the person reached through the trusted
comparison channel; it does not prove a legal or real-world identity.

## Fingerprint definition

The v1 fingerprint binds the case-sensitive PeerId and the raw public key:

```text
SHA-256(
  "fjarsyn-peer-identity-v1\0" ||
  u32be(peer_id_length) ||
  peer_id_utf8 ||
  ed25519_public_key_32
)
```

Fjarsyn displays the complete 256-bit digest as uppercase hexadecimal groups.
Changing either the PeerId or any key bit changes the fingerprint.

## Proof of possession and key replacement

The invite is deliberately not self-signed: a substituted identity could
self-sign its own substituted key, which would not establish human trust.
The listening peer proves possession of the saved key during the pinned TLS
1.3 handshake. The initiating peer proves possession by signing the endpoint
hello, session request and subsequent negotiation envelopes. Signed SDP also
binds the WebRTC DTLS fingerprint to the saved Ed25519 identity. WSS encrypts
the WebSocket request and negotiation contents before the initiator identity,
session ID, SDP or ICE candidates are sent.

This transport does not make a peer invisible on the LAN. mDNS metadata,
addresses, ports, connection timing and volume, the TLS ClientHello and its
observable handshake fingerprint/capabilities, and later ICE/STUN/WebRTC
traffic patterns remain observable.

Replacing a contact key requires importing a new invite for the same PeerId
and repeating the fingerprint comparison.
`contacts::ContactsService` suspends new session admission and
drains any existing authenticated session while the pinned key changes. New
signaling connections immediately require the replacement key as their TLS
pin.

A future QR view may encode the exact same invite as a convenience. Scanning a
QR code would transport the invite but would not replace fingerprint
verification.
