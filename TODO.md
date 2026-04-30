# TODO

Here you can find the current TODOs for the project. The TODOs are approximately listed by priority.

Each TODO is somewhat abstract and may require a lot of work to implement.

- Harden signaling identity and routing.
  - [x] Pin each listener connection to its first advertised peer ID to prevent route poisoning.
  - [x] Drop listener messages addressed to another local peer before registering routes.
  - [x] Generate and persist a local Ed25519 signaling identity key.
  - [x] Add signed signaling envelope primitives with stale-message and replay checks.
  - [x] Add a trusted-peer key directory abstraction for verifier wiring.
  - [x] Add a contacts/database migration that binds peer IDs to trusted public keys.
  - [x] Wire signed envelopes into the signaling transport and reject unsigned/untrusted messages.
  - [x] Add a basic contact import field for trusting peer public keys.
  - [x] Add transport-level unsigned/untrusted/replay regression tests.
  - [x] Add validation and editing for trusted public keys on existing contacts.
  - [x] Add end-to-end spoofing/replay regression tests once transport enforcement exists.
- Finish Windows Graphics Capture device-loss recovery and hardware validation.
  - [x] Recreate the D3D device and WGC frame pool after recoverable DXGI device/access-loss errors instead of only resetting pooled textures.
  - [x] Rebuild the WGC capture session during device-loss recovery instead of keeping a stale session handle.
  - [ ] Exercise device-loss and resize/recreate behavior on real hardware.
- Implement real software pixel conversion before enabling RGBA10, RGBA16 or NV12 preview paths.
- Expand coverage for high-risk behavior.
  - Signaling spoof/replay/rejection cases.
  - Capture resize/device-reset paths.
  - FFmpeg/D3D interop fallback behavior.
  - UI workflow regressions for calls, messaging, and startup recovery.
- Keep clippy clean under `cargo clippy --workspace --all-targets -- -D warnings`.
- Revisit `ui/app/handlers` and consolidate routing/dispatch further if it keeps growing noisier.
- Add more focused lifecycle/startup/retry sequencing tests now that the app event/command boundary is stable.
- Audio capture, streaming and playback.
