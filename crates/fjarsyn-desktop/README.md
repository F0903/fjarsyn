# fjarsyn-desktop

The Windows desktop adapter and Iced user interface for Fjarsyn. This crate
retains the desktop-visible state of the headless `fjarsyn_engine::Engine` and
owns only the desktop lifecycle, platform capture picker, read-only projection
subscriptions, session-media presentation, and the interface. Screen-share
transactions and media pipelines are owned by the engine's
private hosted screen-share implementation; the desktop interacts through
`screen_share::ServiceHandle`, returns the engine's opaque selection token with
each picker result, and projects engine output through one desktop-owned
`EngineAdapter`. One `EngineRuntime` owns the headless `Engine`,
that adapter, and the canonical output receivers for their complete shared
lifetime; startup returns this owner directly without an intermediate
"started application" bundle. Its watch-retained `EngineState` is the latest
desktop-visible aggregate assembled from independent capability streams; it is
neither an exhaustive representation of the engine nor an atomic engine
snapshot. Initial state is read from that aggregate before per-runtime output
receivers become visible, while Iced receives only lightweight
`EngineStateChanged` notifications for later changes. Transient notices and
terminal engine-adapter failures use independent bounded paths. A source
closure or panic therefore moves the shell into an inert, restart-required
state instead of silently freezing part of an otherwise interactive UI.
Startup completions and engine-adapter output carry one process-local runtime
identity, so work queued by an obsolete runtime cannot replace or mutate the
current one. The root update loop routes each top-level message family to one
explicit owner. Active screens receive only screen-scoped messages; the shell
owns application effects, including explicit correlated completion of contact
mutations, without broadcasting or cloning every message through both layers.
The active-screen router also matches every nested screen family exhaustively,
so adding one requires an explicit routing decision.

Frame presentation imports bounded engine-owned GPU texture leases through the
engine's typed D3D12 interop boundary. The desktop caches physical imports by
texture identity, retains each exact frame through submitted GPU work, keeps
viewer uniforms isolated, and uses CPU pixels or a placeholder on import failure.

The desktop also owns `settings.json`, the renderer power preference, and
editable settings drafts; it passes only secret-free network, capture, and
video settings into the engine. If engine startup fails after that document
has loaded and validated, the shell exposes only a failure overview and the
Settings recovery route. `Apply and retry` validates and atomically persists
the draft before installing a fresh runtime instance; validation or storage
failure leaves the failed runtime state and editor intact. Malformed persisted
settings are reported before Iced starts and therefore cannot be repaired by
this in-app recovery path.

See the [peer-session architecture](../../docs/architecture/peer-sessions.md),
[GPU frame-resource contract](../../docs/architecture/gpu-frame-resources.md),
and [Rust module convention](../../docs/architecture/rust-modules.md).
