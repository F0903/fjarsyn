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
Fjarsyn shell
|-- desktop settings (power preference + secret-free engine settings)
|-- ui::runtime::EngineRuntime
|   |-- EngineAdapter -> engine-output coordinator
|   |-- EngineReceivers (canonical retained receiver bundle)
|   `-- fjarsyn_engine::Engine
|       |-- active runtime settings and local public identity
|       |-- private DPAPI-protected local identity store
|       |-- SQLite pool (shared by private capability-owned stores)
|       |-- Services (passive typed capability facade)
|       |   |-- contacts::ContactsService
|       |   |   `-- contacts::Directory -> contacts::SqliteStore
|       |   |-- peer_session::ServiceHandle
|       |   |-- presence::ServiceHandle
|       |   |-- messaging::ServiceHandle
|       |   |-- media::codec::ServiceHandle
|       |   `-- screen_share::ServiceHandle
|       `-- service_host::ServiceHost
|           |-- PeerSessionService (private owner)
|           |   |-- identity-pinned TLS 1.3 WSS signaling listener
|           |   |-- private session registry
|           |   `-- private session actor (one per peer)
|           |       |-- RTCPeerConnection
|           |       |-- reliable ordered control data channel
|           |       |-- reliable ordered messaging data channel
|           |       |-- H.264 media sender and receiver
|           |       `-- owned async/network tasks
|           |-- PresenceService (private owner)
|           |-- MessagingService (private owner) -> messaging::SqliteStore
|           |-- ScreenShareService (private owner)
|           |   |-- local capture/encoding controller
|           |   |-- remote decoding controller
|           |   `-- session/share reconciler
|           `-- CodecService (private owner)
|-- platform capture picker
`-- read-only presence, session, messaging and screen-share projections
```

`fjarsyn_engine::Engine` is the canonical headless aggregate and application
composition root. Engine startup loads the private local identity, opens the
database, and then uses its private `init_services` operation to load and
validate trusted contacts, resolve the session/presence startup cycle through
a private root-level `DeferredResolver`, construct the passive typed `Services`
facade, and give independently executing implementations to a generic
`service_host::ServiceHost`. Capability-owned stores own SQL and
implement private persistence ports; neither their construction nor the
database pool crosses into the desktop crate.

The host-owned, crate-private `PeerSessionService` retains the session runtime.
Its private actor is the only writer of the session registry. A per-peer
session actor is the only owner of its peer connection, channels, state and
child tasks.
Application code uses `peer_session::ServiceHandle` clones containing command
senders and read-only state receivers; they never expose `RTCPeerConnection`,
mutable data-channel slots or shared task collections.

Every network task and normally responsive async task has one owner, a
cancellation path and a bounded, awaited shutdown path. `Drop` may provide
best-effort cancellation but is not the primary lifecycle mechanism.
Synchronous codec work runs on dedicated, service-owned OS threads rather than
Tokio workers. Watchdogs bound the async owner's wait and quarantine an
unresponsive codec direction. A codec supervisor that must retain the native
thread's join handle is deliberately detached at the application deadline;
Rust does not attempt to kill a thread inside FFmpeg. Process isolation remains
follow-up hardening for forcible termination and native-crash containment.

## Engine and capability responsibilities

### `fjarsyn_engine::Engine`

- Retain active secret-free runtime settings, local identity, the database, the typed
  `Services` facade, and the generic service host as the canonical headless
  application boundary.
- Compose concrete capabilities in explicit dependency order through the
  private `init_services` operation.
- Perform bounded, awaited rollback under a fresh rollback deadline when a
  later startup stage fails and retain any rollback failure in `StartError`.
- Prepare codec cancellation as soon as shutdown begins, then stop hosted
  implementations in the declared screen-sharing, codec, presence,
  peer-session, and messaging phases while attributing every failure to its
  service, and finally close the database.
- Consume itself during shutdown so public capability handles are released,
  early codec preparation is synchronous, and one absolute deadline covers
  every hosted shutdown phase plus database connection draining.

### `Services`

- Provide the passive, statically typed capability facade published by Engine.
- Retain `ContactsService` directly and expose one domain-specific
  `ServiceHandle` for every independently executing service.
- Contain no startup recipe, shutdown policy, database ownership, dynamic
  registration, or service lookup.

### `service_host::ServiceHost`

- Retain heterogeneous independently executing service implementations after
  extracting their typed capability handles.
- Stop implementations in Engine's declared phase order and attribute every
  failure to its service.
- Treat a supplied absolute deadline as a hard fence, synchronously cancelling
  the current and all remaining implementations when the shared budget ends.
- Cancel the current and all remaining implementations if an awaited startup
  rollback or shutdown future is dropped.

The generic host erases only lifecycle operations after extracting a typed
handle. Direct services remain ordinary typed fields in `Services`; they do
not acquire a host merely to fit the abstraction. The host contains no
knowledge of Fjarsyn's concrete capability graph and provides no dynamic
lookup, command bus, or service locator.

### Private peer-session hosted implementation

The service host retains the crate-private `PeerSessionService`; application
commands cross the public `peer_session::ServiceHandle`. Construction
configuration, limits, and endpoint/trust resolver ports remain private to
Engine composition. Immutable session state and identifiers, semantic events
and errors, and bounded handle capabilities remain public where callers need
them.

- Initialize the local identity and identity-pinned WSS signaling listener.
- Accept application commands to connect, accept, reject and disconnect.
- Resolve one active or pending session per peer.
- Take one immutable snapshot of the peer's current unauthenticated endpoint
  hints for each explicit Connect command and try a bounded candidate set.
- Resolve simultaneous connection attempts deterministically.
- Verify that every signal matches its registered peer and session.
- Route restart signaling only to the exact authenticated active session, without
  creating another session or incoming-request prompt.
- Publish immutable `peer_session::Sessions` values and semantic
  `peer_session::Event` values.
- Route connected-session commands to the correct per-peer session actor.
- Shut down and join every session and signaling task within the service deadline.

It does not encode frames, persist messages, render UI or expose transport objects.

### Per-peer session actor

- Own one immutable `SessionId`, local identity and remote identity.
- Own one WebRTC peer connection and its media/data endpoints.
- Run the explicit session state machine.
- Send and receive the versioned control and messaging protocols.
- Expose a bounded media-sample capability without exposing WebRTC internals.
- Reject commands and runtime events for the wrong or obsolete session.
- Own ICE-restart attempts and monotonically increasing transport generations.
- Close each temporary signaling path after the required WebRTC capabilities
  open or recover.
- Cancel and join its RTC/network child tasks on disconnect or failure.

### Private presence hosted implementation

- Advertise the local peer and signaling port through mDNS.
- Parse each claimed peer ID once at the mDNS ingress into a validated
  `PeerId`; ignore malformed claims without treating syntactic validity as
  authentication.
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

### `contacts::ContactsService`

- Serialize contact identity writes at the application boundary.
- Suspend peer-session admission and drain any authenticated session before a
  trusted key is changed or removed.
- Restore admission only after the contact cache and store mutation has a
  definitive outcome.
- Prevent screens and other external callers from bypassing that barrier.

### Private messaging hosted implementation

- Persist and project local conversation history.
- Send chat payloads only through a connected peer session.
- Persist incoming messages emitted by authenticated sessions.
- Bind receipts to the authenticated peer and message ID.
- Never dial endpoints, open signaling connections or retry across disconnected sessions.

### Private screen-share hosted implementation

The engine service host retains the crate-private `ScreenShareService`;
application commands and read-only output cross the public
`screen_share::ServiceHandle`. The service owns screen sharing as one session
capability, including local capture/encoding, remote decoding, share
reconciliation, and the transaction that coordinates a media pipeline with
authenticated peer-session share state.

The desktop owns the platform picker because choosing a window or display is an
interactive UI operation. Before opening it, the desktop reserves the engine's
single local capture slot and receives an opaque `screen_share::Selection`.
Picker completion, cancellation, and failure return that exact token, so a
late result cannot affect a newer attempt or a different session. `Selection`
is an RAII lease: dropping its last uncommitted clone cancels the reservation,
while a successfully observed start commits it. Once the user has selected a
capture item, the engine starts or stops the peer-session share and the matching
pipeline as one service-owned operation, including failure recovery and
rollback; the desktop never obtains an `EncodedVideoSink` or assembles that
transaction itself. Only a local user command may begin capture, while
authenticated remote control events may only drive the remote-share side.

The desktop subscribes to read-only presence, session, messaging, and
screen-share projections and presents their states and latest frames. The
desktop `EngineAdapter` owns a coordinator that seeds a retained aggregate
before it starts polling the same watch receivers. The retained receiver
travels with the runtime owner and is read before its Iced subscriptions become
visible, so startup hydration uses the newest available state. Each startup
attempt and all of its engine-adapter output carry one process-local runtime
identity. The shell accepts initialization only from its expected attempt and
accepts adapter output only from the active runtime, so late completion,
shutdown, or process replacement cannot resurrect or mutate an obsolete owner.

Post-start durable state is retained as one watch-backed `EngineState`. It is
the latest desktop-visible aggregate assembled from independent capability
streams, not an exhaustive description of the engine or an atomic snapshot
across those streams. Iced messages carry only an `EngineStateChanged` wake;
the shell then reads the newest retained value, so full presence,
conversation-history, session, or frame states cannot accumulate in an event
FIFO. Intermediate values coalesce inside the watch channel, and duplicate
wakes may only reapply the same current state. Only notifications the desktop
presents cross the transient notice channel: incoming requests, connections,
non-local closes, incoming-message peers, and screen-share failure reasons.
Message bodies, receipts, duplicate durable-state events, and ignored codec
events do not cross that adapter boundary. Terminal engine-adapter failures
have their own capacity-one channel, so source closure or panic cannot sit
behind state or notice backpressure before the Iced boundary. Once forwarded
into Iced, a failure shares the normal UI event queue and moves the UI out of
its interactive ready state into an inert restart-required state;
service-mutating controls never remain active against snapshots that can no
longer advance.

An engine startup failure leaves no live `EngineRuntime`. The desktop admits
only its Home failure overview and Settings recovery route; peer, contact,
messaging, and screen-share actions remain inert. Recovery validation and the
atomic settings write complete before a fresh runtime ID is allocated.
A failed validation or write starts nothing, another engine-start failure
retains the editor and newest error, and success returns to Home. This path
assumes the desktop settings document already parsed and validated: malformed
persisted settings fail before Iced starts and require external correction.

Navigating away must not create, destroy, or duplicate engine media workers.
Presentation code does not own capture providers, codecs, WebRTC capabilities,
or service task handles.

All FFmpeg construction and codec calls execute on dedicated, owned OS threads.
An active call has a ten-second watchdog. A completed call may publish output
only while its originating pipeline and codec direction are still current; a
reply that arrives after timeout, cancellation or replacement is discarded.

Encoder and decoder availability are quarantined independently. If an active
call exceeds its watchdog or its worker is otherwise lost, the affected
direction enters sticky quarantine for the rest of the process lifetime. A
stuck encoder therefore does not disable decoding, and a stuck decoder does not
disable encoding, but no pipeline in the quarantined direction is recreated or
automatically retried. The UI retains a restart-required failure for that
direction across reconciliation and navigation; only restarting Fjarsyn clears
it.

`EngineRuntime` shutdown first cooperatively stops and joins its
`EngineAdapter`, without taking ownership of any underlying capability. The
runtime retains its canonical `EngineReceivers` until that join completes,
then consumes its `Engine` through `Engine::shutdown`. Engine shutdown establishes one
absolute three-second engine deadline and synchronously pre-signals the codec
service so codec initialization cannot leave a screen-share command waiting.
It then stops the hosted `ScreenShareService` before `CodecService`, allowing
its local and remote pipelines to release codec work while the codec capability
is still available. Shutdown continues through presence, sessions, messaging,
and database teardown while budget remains, even if an earlier owner reports
an error. If the deadline expires, the host synchronously cancels the current
and remaining services; the SQLx pool is still marked closed, but connection
draining is not awaited past the fence. Startup rollback uses the same ordered
mechanism with a separate fresh deadline. There is no shared media lock:
callers retain only a cloneable
`screen_share::ServiceHandle`, while Engine's generic service host retains and
shuts down the single `ScreenShareService` owner. Responsive workers close
their bounded inputs, finish cleanup, and are joined. Owners that reach their
deadline detach or cancel unfinished work without awaiting a new cleanup tail;
late output remains suppressed.

GPU-backed preview frames cross the engine/desktop boundary as bounded texture
leases rather than independently paired textures and raw handles. A shared
D3D11 fence orders production before D3D12/wgpu sampling, an exact-submission
completion guard prevents early slot reuse, and separate frame/texture
identities distinguish content from the cached physical import.
Import failure follows an explicit CPU-upload or unavailable-placeholder path.
The complete producer-independent contract is documented in
[`gpu-frame-resources.md`](gpu-frame-resources.md).

A process restart first completes that same owned shutdown and only then lets
the shell launch a replacement. Closing the window while shutdown is still in
progress changes the pending terminal action to exit, so it cannot launch a
replacement process after the user has cancelled the restart.

WGC's synchronous COM close runs on a detached cleanup thread so a stalled
driver cannot block the async deadline. `Drop` is still only an immediate
best-effort cancellation path.

This boundary deliberately does not claim that an in-flight FFmpeg call is
interruptible. Rust cannot safely terminate one native thread. A hung thread
and any FFmpeg, COM or GPU resources it retains remain alive until the call
returns or the operating-system process exits, and an in-process native crash
can still terminate Fjarsyn. A supervised codec child process is required to
forcibly terminate such calls, reclaim their resources and contain crashes.

## State model

Absence from the session registry means disconnected. Live
`peer_session::Phase` values are:

```text
Outgoing: Requesting -> Negotiating -> Connected <-> Reconnecting -> Disconnecting
Incoming: Incoming  -> Negotiating -> Connected <-> Reconnecting -> Disconnecting

Any live phase -> removed from registry on rejection, unrecovered failure or closure
```

`Reconnecting` is recovery of the existing session, not a new session. An
unrecovered failure is an event with a reason followed by removal; failed
connection objects are never placed back into the registry for reuse.

Presence and presentation are derived independently:

```text
Presence: Away | Nearby { endpoint_hints, last_seen }
Session:  Disconnected | Connecting | Incoming | Connected | Reconnecting | Disconnecting
Local share:  Inactive | Selecting | Starting | Active | Stopping | Failed
Remote share: Inactive | Starting | Active | Failed
```

All commands and events after discovery carry the relevant request, attempt or session ID. An event from an obsolete ID cannot mutate current state.

## Signaling protocol

Signaling is temporary and carries negotiation only. The versioned signed payload contains:

- Protocol version.
- Random session ID.
- Exact sender peer ID.
- Exact recipient peer ID; broadcasts are forbidden.
- One of endpoint hello, endpoint proof, request, restart, restart
  acknowledgement, acceptance, offer, answer, ICE candidate, ready, ready
  acknowledgement, rejection or cancellation.
- The corresponding challenge, transport generation, SDP, candidate or bounded
  reason payload.

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

### ICE recovery

A connected session treats WebRTC `Disconnected` as transient first. It keeps
the session in `Connected` for the configured grace period and cancels an
unengaged recovery if the current transport reconnects. Expiry of that grace,
or an explicit ICE `Failed` state, moves the same actor to `Reconnecting`. One
transport loss admits one bounded restart attempt; there is no automatic retry
loop inside that attempt, and one absolute `ice_restart_timeout` covers the
fresh signaling connection, restart negotiation and readiness handshake. A
timeout or failed attempt removes the session unless the old transport
reconnects before the attempt is engaged.

The roles fixed during initial negotiation also govern every restart. Either
peer may detect the loss and open restart signaling, so an answerer-only
failure can recover without waiting for the other ICE agent to notice it. The
original outgoing peer remains the sole SDP offerer and the original incoming
peer remains the answerer. If both peers dial simultaneously, the fixed
offerer's outbound WSS path wins and the answerer aborts and drains its
competing dial. This avoids offer glare without making recovery depend on
symmetric failure detection.

Recovery opens a fresh TLS 1.3 WSS connection using fresh mDNS endpoint hints
and the same exact Ed25519 raw-public-key pin, endpoint proof, frame limits and
authentication deadlines as initial negotiation. A signed `Restart` must name
the exact active `SessionId`, trusted peer and next transport generation. The
listener routes it to that existing actor without inserting a session or
emitting an incoming-session prompt. Unknown, mismatched, stale and
out-of-order restart intents fail closed. The actor also retains the public key
authenticated by the initial session and compares every dialed or attached
restart connection against it, so a resolver change cannot silently replace a
live session's peer identity.

Generation zero identifies initial negotiation. Every accepted restart uses
exactly the next monotonically increasing transport generation. The signed
`Restart`/`RestartAck`, `Offer`, `Answer`, `IceCandidate`, `Ready` and
`ReadyAck` payloads carry that generation, so stale or future signaling cannot
mutate the current transport. Every candidate must also carry the exact ICE
username fragment from that generation's already-installed remote SDP, and a
per-generation cap bounds the complete candidate stream. The offerer waits for
the authenticated `RestartAck` before invoking WebRTC's destructive
`create_offer(ice_restart = true)` operation. Both offer and answer must rotate
their prior username fragment and password; unchanged credentials fail closed.

WebRTC state callbacks are wakeups, not generation proof: before accepting a
connected notification, the actor re-reads the current peer, ICE and DTLS
states and requires both new descriptions plus a selected candidate pair. The
underlying ICE restart synchronously clears the old pair, so that selected pair
must come from connectivity checks after the credential reset. Only then, with
both required data channels still open and the signed `Ready`/`ReadyAck`
handshake complete in both directions, is the restart committed. The actor
returns to `Connected` only after bounded recovery-signaling shutdown.
Signaling is therefore temporary for both initial negotiation and recovery; no
persistent per-contact signaling route is introduced.

Recovery retains the same `SessionId`, session actor, `RTCPeerConnection`, data
channels, media tracks, local and remote `ShareId` state, and engine-owned
screen-share pipelines. While `Reconnecting`, new outbound chat, receipt and
share-control commands are gated. Authenticated inbound application frames are
retained in a bounded buffer and flushed only after successful readiness; an
explicit remote disconnect is delivered immediately. Encoded outbound video
samples are consumed and dropped rather than queued, so stale media is not
replayed after recovery. Existing media ownership and share state resume when
the session returns to `Connected`.

ICE `Failed` is recoverable because ICE credentials and candidate pairs can be
replaced without recreating the session capabilities. A closed ICE or peer
connection, failed or closed DTLS transport, or closed required data channel
is terminal: ICE restart does not attempt to resurrect destroyed DTLS, SCTP or
application endpoints.

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

- `fjarsyn-control-v2`: reliable ordered control messages.
- `fjarsyn-messaging-v2`: reliable ordered chat and receipt messages.
- A pre-negotiated H.264 screen-sharing media track in each direction.

Control and messaging payloads are versioned and size-bounded. DTLS encrypts and authenticates data channels, while DTLS-SRTP protects media. Once the session fingerprint is authenticated through signed signaling, per-message Ed25519 signatures are unnecessary.

Starting a screen share sends a control event and begins writing samples to the existing media track. Stopping sends a control event and stops writing samples. It does not create a call, renegotiate the track or create a new peer connection.

Each `ShareStarted`/`ShareStopped` control event carries both the public
`ShareId` and a non-zero, monotonically increasing `ShareEpoch` for that
sender's session direction. An `EncodedVideoSink` is an immutable capability
bound to that exact pair, so a producer from an old share cannot have a queued
sample relabelled as the current share. The capability is revoked at the share
boundary, preventing an obsolete producer from backpressuring the next share.

The epoch is also encoded as an eight-byte, big-endian value in Fjarsyn's
mandatory RTP header extension. Its dynamic extension ID is negotiated in the
video SDP and must remain stable across ICE restarts. The extension is attached
to every RTP fragment. A missing, zero or malformed epoch is a protocol error.
Before depacketization, the receiver drops packets from lower epochs, continues
the current builder for equal epochs and discards the builder before accepting
a higher epoch. It never moves back to an older epoch.

Depacketized samples retain their epoch through the bounded session media
queue. If media for the next share beats its ordered data-channel control event,
the old decoder pipeline parks without consuming it. Control reconciliation
then replaces the exact `(ShareId, ShareEpoch)` binding and hands the retained
first sample to the new decoder. Decoded frames and local previews retain the
same exact binding through UI projection. SPS gating remains an independent
H.264 bootstrap check, not a share-identity boundary. SRTP authenticates the
RTP header and media; the epoch itself is non-secret metadata and is not
assumed to be encrypted.

## UI model

The canonical route is contact-oriented:

```text
Home
Contacts
Peer { peer_id }
Settings
```

`ui::screens::peer::Screen` shows identity, separate Nearby and Connected
state, local history, a connection action, chat, remote video and local sharing
controls. It projects `Reconnecting` explicitly while retaining the session's
screen-share state. The composer and sharing controls are enabled only when
their connected-session capabilities are ready.

Screens hold only presentation state such as the selected peer, draft text,
selected panel and preview visibility. They do not own services, peer
connections, channels, capture providers, codecs or task handles. Backend
events never hijack navigation.

The desktop settings screen owns an editable draft and persists only desktop
preferences plus secret-free engine runtime settings. The engine owns the
local peer identifier and signing key as one private record; only the peer
identifier and public key are projected outward. See
[`../security/local-data.md`](../security/local-data.md) for the local storage
boundary.

## Dependency rules

- Domain identifiers, states, commands, events and message types do not depend on Iced, SQLx, Windows, FFmpeg or WebRTC.
- Desktop code cannot initialize engine storage or construct capability stores;
  those adapters are private members of `Engine`'s composition graph.
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
- ICE restart repairs an ICE path only. Destruction of the peer connection,
  DTLS transport or required data channels still ends the session.
- Each transport loss has one bounded recovery attempt rather than an
  indefinite retry policy; failure or timeout requires a new deliberate
  session.

## Required verification

- Exhaustive pure session-state transition tests.
- Identity, target, session binding and signaling replay tests.
- Simultaneous-connect convergence tests.
- Out-of-order ICE rejection, exact-username-fragment binding and total
  per-generation candidate-cap tests.
- ICE-restart state tests cover transient-disconnect grace, cancellation before
  engagement, `Connected -> Reconnecting -> Connected`, bounded timeout and
  terminal removal after an unsuccessful attempt.
- Restart protocol tests prove the original outgoing role remains the sole
  offerer even when the answerer initiates recovery or both peers dial,
  `RestartAck` precedes ICE-credential rotation, both descriptions rotate, and
  wrong, replayed or out-of-order transport generations fail closed.
- Restart admission tests require the exact active session, peer and currently
  trusted key and prove that recovery cannot create a session or user prompt.
- Two-peer restart tests preserve the same session ID, actor-owned peer
  connection, data channels, media tracks, share IDs and engine-owned
  screen-share pipelines while application commands are gated, inbound frames
  are bounded, and video samples are dropped during recovery.
- Transport classification tests restart ICE failure but treat closed ICE or
  peer connections, DTLS failure/closure and required-channel closure as
  terminal.
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
- Codec lifecycle tests cover the ten-second active-call watchdog, independent
  encode/decode quarantine, persistent restart-required projection, suppression
  of late output and participation in one shared three-second engine-shutdown
  deadline.
- Screen-share service verification must cover start/stop transaction rollback,
  exact session/share binding, read-only projection output, and shutdown before
  the codec phase after codec cancellation has been prepared.
- Windows GPU verification must apply the same bounded lease, ready-fence,
  draw-completion, import, pressure-drop, and fallback invariants to every
  capture or decoder producer, independent of its native source.
- Repeated connect/disconnect and responsive-worker shutdown tests leave no
  owned tasks or routes; watchdog-timeout tests instead prove bounded
  detachment and quarantine without claiming native-thread reclamation.
- Two-peer loopback tests cover connect, accept/reject, messages, receipts and disconnect.
- Desktop engine-adapter unit tests cover retained-watch coalescing, harmless
  duplicate wakes, stale runtime identities, source closure and panic, and
  clean, failed, or timed-out `EngineAdapter` shutdown. Full shell integration
  must additionally exercise startup recovery and stale-owner shutdown.
