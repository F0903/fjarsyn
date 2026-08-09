# fjarsyn-engine

Fjarsyn's headless application engine. It owns private identity persistence,
pairing, database persistence, presence, peer sessions, messaging,
screen-share orchestration, capture pipelines, and codec execution without
depending on a user-interface framework.

The public `settings` module contains only secret-free runtime settings for
networking, capture, and video. Desktop preferences and settings-file
persistence belong to `fjarsyn-desktop`; the local signing key is loaded from
the engine's private, per-user DPAPI-protected identity store and never enters
the public settings value.

Its public module tree is capability-oriented: `contacts`, `messaging`,
`presence`, `peer_session`, `screen_share`, and `media` each own their service
boundary and any persistence or platform adapters that belong to that
capability. Public callers use direct capability interfaces and typed
`ServiceHandle` values through `Services`; concrete hosted-service owners and
their construction-only configuration remain private Engine implementation
details. The private `ScreenShareService`, for example, owns the local
start/stop transaction and the local and remote media pipelines while callers
interact through `screen_share::ServiceHandle`.

GPU frames are immutable engine-owned resources. Capture and hardware decode
publish the exact shared D3D11 texture together with its ownership, stable
identity, and ready-fence synchronization; consumers never coordinate through
copyable raw handles.

`fjarsyn_engine::Engine::start` is the canonical composition boundary. Engine
constructs the application graph in its private `init_services` operation and
exposes the resulting capability interfaces through the passive, typed
`fjarsyn_engine::Services` facade. A generic
`fjarsyn_engine::service_host::ServiceHost` retains independently executing
service implementations and supplies ordered shutdown, cancellation, and
failure attribution. Screen sharing stops before the codec service, while
Engine prepares codec cancellation early so pipeline shutdown cannot become
stuck behind codec initialization. Engine owns startup rollback and the
database lifecycle; one absolute deadline covers the entire hosted-service and
database shutdown sequence, and startup rollback receives its own fresh
bounded deadline. Concrete SQLite stores remain internal to the engine.

See the [peer-session architecture](../../docs/architecture/peer-sessions.md),
[GPU frame-resource contract](../../docs/architecture/gpu-frame-resources.md),
and [Rust module convention](../../docs/architecture/rust-modules.md).
