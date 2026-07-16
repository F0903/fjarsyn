# Peer session architecture

Status: accepted for the clean development rework.

Fjarsyn is a screen-sharing application built around deliberate, authenticated peer sessions. It is not an offline messenger and does not maintain hidden per-contact connections. There is no compatibility requirement for the previous call-oriented architecture or wire protocol.

## Product model

Four concepts are independent:

- A contact is a locally trusted peer identity.
- Presence indicates that mDNS currently advertises a possible endpoint for that identity.
- A peer session is an explicitly accepted WebRTC connection.
- Messaging and local or remote screen sharing are capabilities inside a connected session.

mDNS presence is a reachability hint, not authentication. Losing an mDNS
advertisement never closes a healthy session. The outgoing signaling transport
first authenticates the listener by pinning its TLS raw public key to the saved
contact identity. Signed signaling then authenticates the initiator and binds
the exact local peer, remote peer, random session ID and WebRTC DTLS
fingerprint.

The UI uses `Nearby`, not `Online`, for mDNS presence.

## Pairing and trust

Contacts are created from a versioned public pairing invite, not from mDNS.
The receiving user must compare the invite's full identity fingerprint over an
independent trusted channel before saving it. Both peers import one another's
invite because incoming signaling also requires a pre-trusted identity.
The saved Ed25519 public key is also the contact's TLS raw-public-key pin; there
is no separate certificate enrollment or hostname trust model.

The invite is a clipboard-friendly text value; a QR renderer is optional and
is not part of the trust model. The exact ceremony, encoding and fingerprint
definition are documented in [`../security/pairing.md`](../security/pairing.md).

## Runtime ownership

```text
UI
`-- application commands and immutable projections
    |-- ContactTrustService -> ContactsService
    |-- PresenceService
    |-- MessagingService
    `-- PeerSessionService
        |-- identity-pinned TLS 1.3 WSS signaling listener
        |-- private session registry
        `-- PeerSession actor (one per peer)
            |-- RTCPeerConnection
            |-- reliable ordered control data channel
            |-- reliable ordered messaging data channel
            |-- H.264 media sender and receiver
            |-- signaling connection during negotiation only
            `-- owned async/network tasks and deterministic shutdown
```

`PeerSessionService` is the application-facing service. Its private actor is the only writer of the session registry. A `PeerSession` actor is the only owner of its peer connection, channels, state and child tasks. Cloneable handles contain command senders and read-only state receivers; they never expose `RTCPeerConnection`, mutable data-channel slots or shared task collections.

Every async/network task has one owner, a cancellation path and a bounded,
awaited shutdown path. `Drop` may provide best-effort cancellation but is not
the primary lifecycle mechanism. A codec call already executing inside
`spawn_blocking` is cooperative rather than forcibly interruptible; that native
FFmpeg boundary is recorded as follow-up hardening.

## Service responsibilities

### PeerSessionService

- Initialize the local identity and identity-pinned WSS signaling listener.
- Accept application commands to connect, accept, reject and disconnect.
- Resolve one active or pending session per peer.
- Take one immutable snapshot of the peer's current unauthenticated endpoint
  hints for each explicit Connect command and try a bounded candidate set.
- Resolve simultaneous connection attempts deterministically.
- Verify that every signal matches its registered peer and session.
- Publish immutable service snapshots and semantic events.
- Route connected-session commands to the correct `PeerSession`.
- Shut down and join every session and signaling task within the service deadline.

It does not encode frames, persist messages, render UI or expose transport objects.

### PeerSession

- Own one immutable `SessionId`, local identity and remote identity.
- Own one WebRTC peer connection and its media/data endpoints.
- Run the explicit session state machine.
- Send and receive the versioned control and messaging protocols.
- Expose a bounded media-sample capability without exposing WebRTC internals.
- Reject commands and runtime events for the wrong or obsolete session.
- Close its temporary signaling path after the required WebRTC capabilities open.
- Cancel and join its RTC/network child tasks on disconnect or failure.

### PresenceService

- Advertise the local peer and signaling port through mDNS.
- Publish nearby endpoint snapshots; `mdns-sd` TTL expiry and removal events
  remove stale advertisements from those snapshots.
- Bound retained presence state by default to 256 peers, four advertisements
  per peer, 16 endpoints per advertisement and 32 aggregate endpoints per
  peer.
- Keep admission stable at capacity: refreshes to an admitted instance remain
  eligible, new peers or instances are ignored, and removal frees capacity.
- Filter unusable endpoint hints, then deterministically de-duplicate and cap
  them while retaining both address families whenever the cap permits it.
- Preserve IPv6 interface indexes supplied by mDNS so scoped link-local
  endpoint hints remain usable.
- Never mutate contacts, create sessions or establish trust.

These limits bound the presence registry and snapshots retained by Fjarsyn.
They do not bound the `mdns-sd` daemon's internal cache or incoming event rate,
and they are not a complete defense against denial of service by a hostile LAN.

### ContactTrustService

- Serialize contact identity writes at the application boundary.
- Suspend peer-session admission and drain any authenticated session before a
  trusted key is changed or removed.
- Restore admission only after the contact cache and repository mutation has a
  definitive outcome.
- Prevent screens and other external callers from bypassing that barrier.

### MessagingService

- Persist and project local conversation history.
- Send chat payloads only through a connected peer session.
- Persist incoming messages emitted by authenticated sessions.
- Bind receipts to the authenticated peer and message ID.
- Never dial endpoints, open signaling connections or retry across disconnected sessions.

### Native session media

The native runtime owns capture selection, encoding, decoding and frame projections for a session. Screens only observe its state. Navigating away must not create, destroy or duplicate media workers. Only a local user command may start local capture; remote control messages can only update remote-share state.

Native async media supervisors are bounded and joined. Closing their bounded
inputs makes codecs exit cooperatively, but an FFmpeg call already running on a
blocking worker cannot be forcibly interrupted by Tokio.

## State model

Absence from the session registry means disconnected. Live session phases are:

```text
Outgoing: Requesting -> Negotiating -> Connected -> Disconnecting
Incoming: Incoming  -> Negotiating -> Connected -> Disconnecting

Any non-terminal phase -> removed from registry on rejection, failure or closure
```

Failures are events with a reason, followed by removal. They are not reusable connection objects.

Presence and presentation are derived independently:

```text
Presence: Away | Nearby { endpoint_hints, last_seen }
Session:  Disconnected | Connecting | Incoming | Connected | Disconnecting
Local share:  Inactive | Selecting | Starting | Active | Stopping | Failed
Remote share: Inactive | Starting | Active
```

All commands and events after discovery carry the relevant request, attempt or session ID. An event from an obsolete ID cannot mutate current state.

## Signaling protocol

Signaling is temporary and carries negotiation only. The versioned signed payload contains:

- Protocol version.
- Random session ID.
- Exact sender peer ID.
- Exact recipient peer ID; broadcasts are forbidden.
- One of endpoint hello, endpoint proof, request, acceptance, offer, answer,
  ICE candidate, ready, ready acknowledgement, rejection or cancellation.
- The corresponding challenge, SDP, candidate or bounded reason payload.

The signed SDP binds the WebRTC DTLS fingerprint to the trusted Ed25519 contact identity. Signaling never carries chat, receipts, screen-share state or media.

The initiating peer opens a short-lived TLS 1.3 WSS connection to a nearby
endpoint. Each explicit Connect command reads one immutable snapshot of the
peer's current mDNS endpoint hints. The connection planner filters unusable
addresses, stably de-duplicates the remaining hints and caps the candidate set
(six by default), reserving a slot for each IP family when both are available.
It tries candidates sequentially with a bounded timeout (two seconds per
candidate by default), while the existing service-operation deadline still
bounds the complete Connect operation. The hints remain unauthenticated
throughout selection and never establish peer identity.

For every candidate, TLS must present and prove possession of the exact
[RFC 7250](https://www.rfc-editor.org/rfc/rfc7250) Ed25519 raw public key derived
from the contact's saved identity before Fjarsyn sends a WebSocket request or
any signaling envelope. A wrong key, plaintext endpoint, TLS failure or
protocol mismatch fails that candidate; fallback candidates use the same pin
and there is no plaintext retry.

After the listener is authenticated and the WSS upgrade completes, the
initiator sends a signed `EndpointHello` with a fresh random challenge and
verifies the matching signed `EndpointProof` against the contact's expected
trusted key. This application layer authenticates the initiating peer to the
listener and retains explicit peer, target, session and replay binding. Only
the authenticated winning connection sends the single `Request` that enters
normal session admission, so failed or malicious endpoint hints cannot create
duplicate user prompts.

The signaling listener binds one dual-stack IPv6 wildcard socket, explicitly
configured to accept both IPv6 and IPv4 connections on the same port, and
wraps every admitted TCP stream in TLS before WebSocket processing. Scoped
IPv6 endpoint URLs retain the interface index learned from mDNS. The temporary
signaling connection closes after the peer connection and required data
channels are ready. Failure to create the listener or its TLS configuration is
an explicit service-startup error rather than a silent degraded mode.

The initial implementation treats a terminal transport failure as the end of
the session; reconnecting creates a fresh authenticated session. An ICE-restart
protocol can be added later without making signaling persistent.

TLS 1.3 encrypts the WebSocket upgrade, session identifiers, SDP, ICE
candidates and all signed signaling envelopes. Fjarsyn uses the existing
long-lived Ed25519 identity through a narrow rustls signer and requires the
contact's exact raw SubjectPublicKeyInfo value; Web PKI roots, DNS names, TOFU
and self-signed X.509 certificates are not trust authorities. TLS 1.2, SNI,
session resumption, early data and plaintext downgrade are disabled, and both
ends require the HTTP/1.1 ALPN used by the WebSocket upgrade.

This does not hide all network metadata. Passive observers can still see mDNS
advertisements, source and destination addresses, ports, timing, traffic sizes,
the TLS ClientHello and its observable handshake fingerprint/capabilities, and
later ICE/STUN/WebRTC traffic patterns. SNI is disabled, so the TLS handshake
does not expose a peer name. Session chat and
control data are separately encrypted by WebRTC DTLS, and media is protected
by DTLS-SRTP.

The listener applies bounded frames, handshake and idle timeouts, connection
limits and authentication-failure closure. Before TLS processing, every
admitted signaling attempt is charged to two token buckets: a global bucket
with a burst of 64 and one-token-per-100-millisecond refill, and a bucket for
the canonical source IP with a burst of eight and one-token-per-500-millisecond
refill. Successful, failed and trusted-peer attempts receive no refund or
bypass. Per-IP bookkeeping retains at most 4,096 sources; a new source fails
closed when that tracking capacity is exhausted. The source socket address is
only a rate-limit key and is never treated as identity.

One absolute authentication deadline covers TLS, the WebSocket upgrade, the
hello and proof, trust resolution, the request and routing. A peer cannot hold
a connection permit for a fresh full timeout at every stage.

This bounds the pre-authentication work admitted by Fjarsyn, not all work in
the operating-system network stack or every form of denial of service on the
LAN.

## WebRTC protocols

The peer connection creates these endpoints during initial negotiation:

- `fjarsyn-control-v1`: reliable ordered control messages.
- `fjarsyn-messaging-v1`: reliable ordered chat and receipt messages.
- A pre-negotiated H.264 screen-sharing media track in each direction.

Control and messaging payloads are versioned and size-bounded. DTLS encrypts and authenticates data channels, while DTLS-SRTP protects media. Once the session fingerprint is authenticated through signed signaling, per-message Ed25519 signatures are unnecessary.

Starting a screen share sends a control event and begins writing samples to the existing media track. Stopping sends a control event and stops writing samples. It does not create a call or a new peer connection.

Encoded samples are currently session-scoped rather than tagged with a
`ShareId` media epoch. Receiver source retention and SPS gating protect normal
stop/restart transitions, but a repeated SPS/IDR in an old buffered tail could
be attributed to a rapidly started new share. A media-epoch tag or explicit
keyframe-boundary handshake is required to eliminate that narrow ambiguity.

## UI model

The canonical route is contact-oriented:

```text
Home
Contacts
Peer { peer_id }
Settings
```

`PeerScreen` shows identity, separate Nearby and Connected state, local history, a connection action, chat, remote video and local sharing controls. The composer and sharing controls are enabled only when their connected-session capabilities are ready.

Screens hold only presentation state such as the selected peer, draft text, selected panel and preview visibility. They do not own services, peer connections, channels, capture providers, codecs or task handles. Backend events never hijack navigation.

## Dependency rules

- Domain identifiers, states, commands, events and message types do not depend on Iced, SQLx, Windows, FFmpeg or WebRTC.
- UI code cannot select network endpoints or access transport handles.
- Persistence code cannot create network activity.
- Presence code cannot establish trust.
- WebRTC callbacks emit typed internal events rather than mutating shared public state.
- Traits exist at genuine replaceable boundaries, not around every concrete object.
- Protocol parsing validates versions, identities, session IDs and size limits before dispatch.

## Clean-break policy

The following concepts are removed rather than adapted:

- Calls, dialing, accepting calls and hanging up.
- Messaging over signaling or address-based messaging commands.
- Persistent per-contact signaling routes.
- Manual contact addresses as session authority.
- Reusable mutable global WebRTC connection state.
- Screen-owned connection and media workers.
- Compatibility aliases, old protocol variants and parallel legacy paths.

The development database schema and protocol start clean. Existing development data may be discarded.
The reworked schema uses its own `fjarsyn-peer-sessions.db` file, leaving the
pre-rework development database untouched rather than carrying migration
compatibility into the new architecture.

## Known transport limitations

- WSS hides signaling contents but not mDNS, IP/port, timing, volume or
  ICE/STUN/WebRTC traffic metadata.
- A failed transport ends the session; ICE restart is not implemented.

## Required verification

- Exhaustive pure session-state transition tests.
- Identity, target, session binding and signaling replay tests.
- Simultaneous-connect convergence tests.
- Early ICE queuing tests.
- Messaging requires the correct connected session and peer.
- Remote control cannot start local capture.
- mDNS removal cannot close a connected session.
- Presence-flood tests enforce peer, advertisement and endpoint cardinality,
  stable admission and capacity recovery after removal, unusable-hint
  filtering, and address-family preservation under endpoint caps.
- Signaling-admission tests cover global and canonical-source-IP token buckets,
  deterministic refill, no refunds or trusted-peer bypass, bounded fail-closed
  source tracking, and rejection before TLS, WebSocket or trust-resolution work.
- Signaling transport tests cover exact Ed25519 raw-key pinning and possession,
  TLS 1.3 and ALPN enforcement, plaintext rejection, absence of application
  bytes before server authentication, IPv4/IPv6 operation and wrong-key
  endpoint fallback without duplicate requests.
- Repeated connect/disconnect and shutdown tests leave no owned tasks or routes.
- Two-peer loopback tests cover connect, accept/reject, messages, receipts and disconnect.
- UI projection tests cover every presence/session combination without navigation side effects.
