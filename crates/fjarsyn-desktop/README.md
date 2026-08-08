# fjarsyn-desktop

The Windows desktop adapter and Iced user interface for Fjarsyn. This crate
retains the headless `fjarsyn_engine::Engine` aggregate and owns only the
desktop lifecycle, platform capture picker, read-only projection
subscriptions, session-media presentation, and the interface. Screen-share
transactions and media pipelines are owned by the engine's
`screen_share::ScreenShareService`; the desktop returns the engine's opaque
selection token with each picker result and projects latest-frame output
without queueing full frame snapshots or hosting a second service runtime.

See the [peer-session architecture](../../docs/architecture/peer-sessions.md)
and [Rust module convention](../../docs/architecture/rust-modules.md).
