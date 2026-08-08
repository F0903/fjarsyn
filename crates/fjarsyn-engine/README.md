# fjarsyn-engine

Fjarsyn's headless application engine. It owns identity, pairing, database
persistence, presence, peer sessions, messaging, screen-share orchestration,
capture pipelines, and codec execution without depending on a user-interface
framework.

Its public module tree is capability-oriented: `contacts`, `messaging`,
`presence`, `peer_session`, `screen_share`, and `media` each own their service
boundary and any persistence or platform adapters that belong to that
capability. `screen_share::ScreenShareService` owns the local start/stop
transaction and the local and remote media pipelines; applications use its
typed `screen_share::ServiceHandle` through `Services`.

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

See the [peer-session architecture](../../docs/architecture/peer-sessions.md)
and [Rust module convention](../../docs/architecture/rust-modules.md).
