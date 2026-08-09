# TODO

This file contains only active work. Completed rework and audit history remains
available in version control and in the architecture documents. Items are
approximately ordered by architectural and release risk.

## Current rework findings

Findings from the full architecture and maintainability scan on 2026-08-08.

### Media ownership and pipeline correctness

- [ ] Measure and tune the six-slot GPU frame pools under realistic load.
  - Record committed GPU memory, native handles, pool-pressure drops and draw-completion latency at 1080p and 4K with concurrent local/remote producers.
  - Use those measurements to confirm or adjust the fixed capacity and the desktop's 32-import cache target without weakening nonblocking backpressure.
- [ ] Make encoded-video loss explicitly keyframe-aware.
  - Never silently discard an arbitrary encoded H.264 frame and continue publishing dependent P-frames as if the stream were intact.
  - Retain the WebRTC sender and consume RTCP loss feedback so the encoder can force a new IDR.
  - After a discontinuity, withhold dependent input until the decoder has a valid SPS/PPS plus keyframe boundary.
- [ ] Extract local and remote screen-share pipeline construction into real pipeline owners.
  - Keep controllers focused on selection, bindings, durable state and reconciliation.
  - Make capture teardown an exactly-once ownership transition; normal stop currently can schedule detached cleanup more than once for the same provider.
  - Isolate synchronous WGC/D3D/COM setup and teardown behind a dedicated capture host thread so a stalled driver call cannot pin a Tokio worker beyond the advertised deadline.
- [ ] Introduce a validated `FrameLayout`/dimension boundary used by capture, codec and GPU-import paths.
  - Reject non-positive or excessive dimensions before casts or allocation.
  - Use checked pixel counts, exact plane sizes and strides for formats such as NV12, and explicit allocation ceilings.
  - Drain all FFmpeg outputs per input, distinguish `EAGAIN`, EOF and terminal errors in both directions, and flush delayed output during shutdown.
  - Add codec contract tests for `EAGAIN`, multiple outputs, delayed output and terminal errors.
- [ ] Propagate WGC capture-item closure and terminal capture failures into screen-share state instead of leaving an active but permanently frozen share.

### Targeted organization follow-ups

- [ ] Split accepted-connection TLS/WebSocket authentication and routing out of the negotiation listener; keep socket-set ownership, admission and listener lifecycle together.
- [ ] Move the tests in `screen_share/shares.rs` after the production declarations (or into a cohesive tests module) so `SessionMedia` and `Shares` remain contiguous.
- [ ] Replace FFmpeg-backend inherent implementations on the public `TranscodeType` with backend-owned mapping functions or an `EncoderInfo` lookup beside the backend.
- [ ] Move very large inline UI test suites, such as contacts workflow scenarios, into cohesive test modules while leaving directly-bound production helpers and implementations with their owning types.

## Remaining engineering work

### Media capabilities and native isolation

- [ ] Implement real hardware scaling and pixel-format conversion for NVENC; use direct texture copies only when source and destination dimensions and formats match exactly.
- [ ] Make hardware encoder/decoder capability probing and fallback explicit.
  - Preserve or restore CPU readback when a configured hardware encoder falls back to software.
  - Match the desktop renderer backend/DXGI adapter to capture and decoder resources, or dynamically switch to CPU/software output after persistent typed import failures.
  - Track capture-device generations and rebuild dependent codecs/importers after WGC device recovery; an encoder must never copy a new-device frame through its stale device context.
- [ ] Run codec workers in supervised child processes so an already-running FFmpeg FFI call can be forcibly terminated and native faults are contained.
- [ ] Stop publishing and projecting local preview frames at capture rate while the preview is hidden.
- [ ] Add correct CPU preview conversion for high-bit-depth and planar formats before advertising software-preview support for RGBA10, RGBA16 or NV12.

### Privacy and local data

- [ ] Decide and document the local chat-history privacy model.
  - Define retention, user deletion, database/WAL handling and the forensic limits of best-effort deletion.
  - Add database encryption or encryption-key destruction if the product requires meaningful local confidentiality.

### Dependencies, portability and release gates

- [ ] Resolve the active unmaintained `bincode` and `ttf-parser` dependency paths by replacing the upstream path, upgrading when upstream migrates, or recording a time-bounded policy waiver.
- [ ] Add `cargo audit`/OSV and dependency-policy checks to CI for the resolved build graph; document why lockfile-only inactive packages do not represent shipped code.
- [ ] Check in a `vcpkg.json` with a baseline so the exact native FFmpeg build is reproducible and auditable.
- [ ] Pin a stable Rust toolchain version and declare the supported `rust-version`; the workspace currently builds on stable without source-level nightly features.
- [ ] Add CI for formatting, workspace/all-target tests, documentation tests, clippy with warnings denied and a locked release build.
- [ ] Enforce the documented Windows-only target through manifest metadata, target-scoped dependencies and a clear unsupported-target compile error.

### Verification

- [ ] Add Windows adapter-backed coverage for the shared GPU-frame contract
  across capture and decoder producers: pooled import and exact slot recycling,
  D3D11-fence-to-D3D12-queue ordering, resource and handle lifetime across
  reset/recovery, bounded pool pressure, and CPU-upload/placeholder fallback. Also cover WGC
  resize/device loss/source closure, hardware codec scaling/fallback and
  repeated capture teardown.
- [ ] Add focused screen-share controller/runtime tests for start rollback, pipeline failure, exact-once teardown and shutdown ordering.
- [ ] Add full shell integration tests for `EngineAdapter`/coordinator failure and disposal of stale successful initialization owners.
- [ ] Add peer-session integration tests proving live global/per-IP admission permits remain held and are released correctly, and that stale RTC callbacks cannot affect a replacement/restarted transport.

## Future features

- [ ] Audio capture, streaming and playback.
