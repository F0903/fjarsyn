# TODO

Here you can find the current TODOs for the project. The TODOs are approximately listed by priority.

Each TODO is somewhat abstract and may require a lot of work to implement.

## Explicit peer-session rework

Clean-break implementation of the accepted architecture in
`docs/architecture/peer-sessions.md`. The old call, address-based messaging and
global WebRTC paths are removed rather than supported in parallel.

- [x] Define the peer-session ownership, state, protocol and UI boundaries.
- [x] Make contacts trusted identities only; remove saved network addresses.
- [x] Separate unauthenticated mDNS presence into an owned `PresenceService`.
- [x] Implement the actor-owned `PeerSessionService` and per-peer WebRTC sessions.
- [x] Move chat and receipts exclusively onto the authenticated messaging data channel.
- [x] Keep screen sharing on WebRTC media tracks under an application-owned media runtime.
- [x] Replace call/message screens with the contact-oriented `PeerScreen` flow.
- [x] Delete all legacy call, signaling-chat and global mutable WebRTC paths.
- [x] Pass state-machine, protocol, persistence, lifecycle, UI projection and loopback tests.

### Peer-session follow-ups

- [x] Add reciprocal, versioned text pairing invites with explicit full-fingerprint comparison over an independent trusted channel. A QR renderer remains optional transport only.
- [x] Add bounded fallback across all current mDNS endpoint hints and an IPv6 signaling listener. Presence hints remain unauthenticated, so never treat endpoint selection as identity.
- [x] Add an application-level cap for presence peers/advertisements and explicit signaling authentication rate limits.
- [x] Encrypt signaling with TLS 1.3-only WSS, pin the listener's RFC 7250 Ed25519 raw public key to the trusted contact identity and retain signed initiator/session authentication with no plaintext fallback.
- [ ] Add ICE restart; a terminal transport failure currently closes the session and reconnect creates a new one.
- [ ] Tag encoded media with a `ShareId`/epoch, or add a keyframe-boundary handshake, so buffered tail frames can never cross a rapid share restart.
- [ ] Isolate FFmpeg codec calls behind a truly interruptible worker/process boundary. Async supervisors are bounded, but an in-flight `spawn_blocking` FFI call exits cooperatively.

- Exercise Windows Graphics Capture device-loss and resize/recreate behavior on real hardware.
- Implement real software pixel conversion before enabling RGBA10, RGBA16 or NV12 preview paths.
- Expand coverage for high-risk behavior.
  - Capture resize/device-reset paths.
  - FFmpeg/D3D interop fallback behavior.
  - UI workflow regressions for peer sessions, messaging, and startup recovery.
- Keep clippy clean under `cargo clippy --workspace --all-targets -- -D warnings`.
- Revisit `ui/shell/handlers` and consolidate routing/dispatch further if it keeps growing noisier.
- Add more focused lifecycle/startup/retry sequencing tests now that the app event/command boundary is stable.
- Audio capture, streaming and playback.

## Audit remediation (2026-07-15)

Findings from the full project security, correctness, media-pipeline, dependency and build audit. Items are grouped approximately by release risk.

### P0 - Security and session integrity

- [x] Move chat, receipts and screen-share control onto encrypted WebRTC data channels.
- [x] Bind signed signaling, answers, ICE candidates, rejection and readiness to the exact local peer, remote peer and random session ID; reject replay.
- [x] Bind data-channel messages and receipts to the authenticated session/peer capability that delivered them.
- [x] Keep outgoing sessions in `Requesting`/`Negotiating` until authenticated negotiation, DTLS and required channels complete; enforce phase timeouts.
- [x] Bound signaling frames, handshakes, idle time, global/per-IP connections and authentication-failure closure.
- [x] Store contacts as peer ID plus required trusted key only; never persist an mDNS address as identity.
- [x] Expose a copyable canonical pairing invite and its full identity fingerprint.
- [x] Require a parsed invite and explicit independent-channel fingerprint confirmation before contact creation or key replacement.
- [x] Add TLS 1.3 WSS signaling confidentiality with exact peer-identity pinning, bounded authentication and downgrade regression coverage.
- [x] Add explicit authentication-attempt rate limiting in addition to connection limits.

### P1 - Lifecycle, session state and media correctness

- [x] Give signaling, WebRTC and listener tasks explicit ownership, cancellation and bounded joined shutdown.
- [x] Replace mutable reusable peer connections with one actor-owned connection per immutable session.
- [x] Serialize connect, accept, reject and disconnect operations and tag transport events with an actor generation so stale callbacks cannot affect a newer session.
- [x] Treat transient WebRTC `Disconnected` separately from terminal `Failed`/`Closed`, with a bounded recovery grace period.
- [x] Queue early ICE candidates until the corresponding remote description has been installed, and report candidate-application failures.
- [ ] Replace copyable raw shared GPU handles with owned RAII handles that always call `CloseHandle`; keep the underlying texture alive for every submitted frame.
- [ ] Redesign D3D11-to-D3D12/wgpu texture sharing around a documented synchronization contract.
  - Use the correct shared-resource flags.
  - Add per-slot keyed-mutex or fence synchronization and explicit frame leases before reusing ring-buffer textures.
- [ ] Implement real hardware scaling and pixel-format conversion for NVENC; use direct texture copies only when source and destination dimensions and formats match exactly.
- [ ] Make hardware encoder/decoder capability probing and failure explicit.
  - Preserve or restore CPU readback when falling back to software.
  - Track capture-device generations and rebuild dependent encoders/importers after WGC device recovery.
- [ ] Make `BufferPool` initialization sound: never expose a safe slice before all bytes are initialized, and use exact checked sizes for planar formats such as NV12.
- [x] Surface capture closure, encoder/decoder initialization failure and worker termination to session/UI state instead of leaving an apparently active but frozen stream.
- [ ] Correct the FFmpeg send/receive state machines: drain all available frames/packets, distinguish `EAGAIN` from terminal errors and flush delayed data during shutdown.
- [x] Move synchronous codec work off ordinary Tokio workers onto `spawn_blocking` tasks with bounded async supervision.
- [ ] Make an already-running FFmpeg FFI call forcibly interruptible rather than relying on cooperative input closure.
- [ ] Tag media with a share epoch so rapid stop/restart cannot attribute an old buffered SPS/IDR tail to a new `ShareId`.

### P2 - Privacy, resilience and resource limits

- [ ] Store the Ed25519 private key using Windows-protected secret storage, with restrictive permissions and atomic temp-file/fsync/rename recovery where files remain necessary.
- [ ] Decide and document the local chat-history privacy model; add database encryption, retention controls and secure deletion if local confidentiality is required.
- [x] Add checked limits for chat/control payloads, signaling frames and bitrate conversions.
- [ ] Add strict decoded-video dimension, pixel-count and allocation limits before codec/frame allocation.
- [x] Tie delivery receipts to the authenticated conversation peer and session, not only a message UUID.
- [x] Persist distinct pending, sent, delivery-unknown, delivered and failed outcomes without unsafe automatic retries.
- [x] Buffer only the bounded final-readiness race and otherwise explicitly reject application/control data before required channels are ready.
- [ ] Give each GPU frame viewer its own aspect-ratio uniform state, or use correctly isolated dynamic offsets for multi-view rendering.
- [ ] Surface GPU import failures and provide a controlled CPU fallback instead of rendering a silent blank frame.
- [ ] Stop hidden local previews from continuing to drive capture-rate UI updates and rendering work.
- [ ] Replace capture-picker `yield_now` busy polling with event-driven waiting or bounded backoff.
- [x] Refresh discovery metadata and remove advertisements on `mdns-sd` TTL/removal events.
- [x] Bound application-level presence registry/advertisement cardinality under a LAN flood.
- [x] Check affected-row counts for contact update/delete operations so stale IDs cannot leave the UI cache inconsistent with SQLite.
- [x] Use checked arithmetic when converting configured bitrate units.

### P3 - Dependencies, portability and release gates

- [ ] Triage and upgrade the RustSec-flagged dependency paths for `quick-xml`, `rsa`, `bincode`, `paste` and `ttf-parser`; document any target-inactive or accepted residual risk.
- [ ] Add `cargo audit`/OSV and dependency-policy checks to CI, including all supported target graphs.
- [ ] Check in a `vcpkg.json` with a baseline so the exact native FFmpeg build is reproducible and auditable.
- [ ] Pin the Rust toolchain instead of following floating `nightly`; determine whether nightly is still required.
- [ ] Add CI for formatting, workspace/all-target tests, documentation tests, clippy with warnings denied and a locked release build.
- [ ] Add native/UI tests and hardware-backed integration coverage for capture, resize/device loss, zero-copy synchronization, codec fallback, source closure and repeated/multi-peer session workflows.
- [ ] Either make non-Windows capture imports compile correctly behind target gates or explicitly document and enforce Windows-only support in manifests and setup instructions.
- [ ] Expand regression tests for peer/session binding, replay handling, listener resource limits, early ICE, lifecycle shutdown and stale-event isolation.
