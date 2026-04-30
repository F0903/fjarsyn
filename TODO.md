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
