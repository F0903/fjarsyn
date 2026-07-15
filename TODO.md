# TODO

Here you can find the current TODOs for the project. The TODOs are approximately listed by priority.

Each TODO is somewhat abstract and may require a lot of work to implement.

- Exercise Windows Graphics Capture device-loss and resize/recreate behavior on real hardware.
- Implement real software pixel conversion before enabling RGBA10, RGBA16 or NV12 preview paths.
- Expand coverage for high-risk behavior.
  - Capture resize/device-reset paths.
  - FFmpeg/D3D interop fallback behavior.
  - UI workflow regressions for calls, messaging, and startup recovery.
- Keep clippy clean under `cargo clippy --workspace --all-targets -- -D warnings`.
- Revisit `ui/app/handlers` and consolidate routing/dispatch further if it keeps growing noisier.
- Add more focused lifecycle/startup/retry sequencing tests now that the app event/command boundary is stable.
- Audio capture, streaming and playback.

## Audit remediation (2026-07-15)

Findings from the full project security, correctness, media-pipeline, dependency and build audit. Items are grouped approximately by release risk.

### P0 - Security and call integrity

- [ ] Encrypt chat and signaling payloads end to end while retaining signed-envelope authentication.
  - Use a peer-authenticated protocol such as Noise/X25519 plus AEAD, or move messaging to the WebRTC DTLS data channel.
  - If WSS is used, add peer identity pinning rather than relying on transport encryption alone.
- [ ] Bind every answer, ICE candidate, decline, receipt and control message to both the expected remote peer and a random per-call/session ID.
- [ ] Complete the peer-pairing trust flow.
  - Add a fingerprint, QR-code or equivalent public-key verification ceremony.
  - Fix nearby-peer contact creation, which currently omits the required trusted public key.
  - Do not persist an mDNS-discovered address until an authenticated connection succeeds.
- [ ] Keep outbound calls in `Dialing` until an authenticated answer and usable WebRTC connection are established; add a failure timeout.
- [ ] Harden the public signaling listener with handshake and idle timeouts, global and per-IP connection limits, frame/message limits, authentication-failure disconnects and rate limiting.

### P1 - Lifecycle, call state and media correctness

- [ ] Give signaling, WebRTC and listener tasks explicit ownership, cancellation and joined shutdown; remove the strong self-retaining `Arc`/channel lifecycle.
- [ ] Fix repeated outbound calls so peer-connection replacement cannot clear the newly selected remote peer or emit signaling with `to: None`.
- [ ] Serialize dial, accept, decline and hangup operations and tag transport events with a call generation so stale callbacks cannot affect a newer call.
- [ ] Treat transient WebRTC `Disconnected` separately from terminal `Failed`/`Closed`, with an appropriate recovery grace period.
- [ ] Queue early ICE candidates until the corresponding remote description has been installed, and report candidate-application failures.
- [ ] Replace copyable raw shared GPU handles with owned RAII handles that always call `CloseHandle`; keep the underlying texture alive for every submitted frame.
- [ ] Redesign D3D11-to-D3D12/wgpu texture sharing around a documented synchronization contract.
  - Use the correct shared-resource flags.
  - Add per-slot keyed-mutex or fence synchronization and explicit frame leases before reusing ring-buffer textures.
- [ ] Implement real hardware scaling and pixel-format conversion for NVENC; use direct texture copies only when source and destination dimensions and formats match exactly.
- [ ] Make hardware encoder/decoder capability probing and failure explicit.
  - Preserve or restore CPU readback when falling back to software.
  - Track capture-device generations and rebuild dependent encoders/importers after WGC device recovery.
- [ ] Make `BufferPool` initialization sound: never expose a safe slice before all bytes are initialized, and use exact checked sizes for planar formats such as NV12.
- [ ] Surface capture closure, encoder/decoder initialization failure and worker termination to call/UI state instead of leaving an apparently active but frozen stream.
- [ ] Correct the FFmpeg send/receive state machines: drain all available frames/packets, distinguish `EAGAIN` from terminal errors and flush delayed data during shutdown.
- [ ] Move synchronous codec work off ordinary Tokio workers onto dedicated threads or `spawn_blocking` tasks.

### P2 - Privacy, resilience and resource limits

- [ ] Store the Ed25519 private key using Windows-protected secret storage, with restrictive permissions and atomic temp-file/fsync/rename recovery where files remain necessary.
- [ ] Decide and document the local chat-history privacy model; add database encryption, retention controls and secure deletion if local confidentiality is required.
- [ ] Add checked limits for chat/control payload size, signaling frames, decoded video dimensions, pixel counts, allocation sizes and bitrate conversions.
- [ ] Tie delivery receipts to the authenticated conversation peer, not only a message UUID.
- [ ] Distinguish queue acceptance from successful WebSocket delivery; add write acknowledgements, retry deadlines and terminal failure state for outgoing messages.
- [ ] Queue or explicitly reject control events while the data channel is not open instead of silently dropping them.
- [ ] Give each GPU frame viewer its own aspect-ratio uniform state, or use correctly isolated dynamic offsets for multi-view rendering.
- [ ] Surface GPU import failures and provide a controlled CPU fallback instead of rendering a silent blank frame.
- [ ] Stop hidden local previews from continuing to drive capture-rate UI updates and rendering work.
- [ ] Replace capture-picker `yield_now` busy polling with event-driven waiting or bounded backoff.
- [ ] Refresh all discovery metadata, including hostnames, when an existing peer is rediscovered; expire stale endpoints.
- [ ] Check affected-row counts for contact update/delete operations so stale IDs cannot leave the UI cache inconsistent with SQLite.
- [ ] Use checked arithmetic when converting configured bitrate units.

### P3 - Dependencies, portability and release gates

- [ ] Triage and upgrade the RustSec-flagged dependency paths for `quick-xml`, `rsa`, `bincode`, `paste` and `ttf-parser`; document any target-inactive or accepted residual risk.
- [ ] Add `cargo audit`/OSV and dependency-policy checks to CI, including all supported target graphs.
- [ ] Check in a `vcpkg.json` with a baseline so the exact native FFmpeg build is reproducible and auditable.
- [ ] Pin the Rust toolchain instead of following floating `nightly`; determine whether nightly is still required.
- [ ] Add CI for formatting, workspace/all-target tests, documentation tests, clippy with warnings denied and a locked release build.
- [ ] Add native/UI tests and hardware-backed integration coverage for capture, resize/device loss, zero-copy synchronization, codec fallback, source closure and repeated/multi-party call workflows.
- [ ] Either make non-Windows capture imports compile correctly behind target gates or explicitly document and enforce Windows-only support in manifests and setup instructions.
- [ ] Add regression tests for signaling confidentiality, peer/session binding, replay handling, listener resource limits, early ICE, lifecycle shutdown and stale-event isolation.
