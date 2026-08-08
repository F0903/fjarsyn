# Rust module structure

Fjarsyn uses the module tree as the primary source of type context.

This document is the governing convention for Rust naming, module ownership,
source-file placement, and public module facades throughout the workspace.

- A `mod.rs` file primarily documents the module, declares child modules, and
  exposes the module's deliberate public facade. Small module-wide constants,
  aliases, and helpers may live there when they have no more natural owner and
  giving them separate files would only fragment the module.
- A module with child modules uses a directory `mod.rs` facade. Do not combine
  a parent `foo.rs` module file with a parallel `foo/` child directory.
- The directory tree mirrors ownership and lifecycle boundaries. Cohesive
  clusters such as `backend`, `registry`, `transport`, `bootstrap`, `model`,
  or `actor` belong in focused submodules instead of accumulating in a flat
  parent directory.
- Shared helpers live under the domain that owns their semantics, or in one
  focused private module when genuinely cross-cutting. Avoid catch-all
  `utils`, `helpers`, or `common` buckets.
- Engine-root modules represent application capabilities such as `contacts`,
  `messaging`, `presence`, `screen_share`, and `media`. Their models, actors,
  transports, and persistence adapters stay with the capability that owns
  their semantics; repositories and implementations are not collected into
  generic role-based buckets. In particular, the independently executing
  screen-share orchestrator belongs to `screen_share`; platform capture-source
  selection and frame presentation remain desktop concerns.
- The `service_host` module is deliberate cross-cutting lifecycle
  infrastructure, not a capability bucket or application service registry.
  `HostedService` defines typed-handle extraction, shutdown, and cancellation
  for orchestrators that execute independently; `ServiceHost` retains those
  implementations and erases only their lifecycle operations. Its optional
  absolute deadline is a hard fence: the host cancels the current and all
  remaining implementations once the shared budget is exhausted. Concrete
  services remain in their capability modules. Engine-specific composition,
  direct services, and the public service facade do not belong in
  `service_host`, and the host provides no dynamic lookup.
- `fjarsyn_engine::Engine` is the canonical application aggregate. It owns
  active configuration, identity and persistence state, the generic service
  host, and the application lifecycle. Its private `init_services` operation
  is the explicit composition root for the concrete capability graph. The
  root-level `Services` type is a passive, typed facade containing direct
  services and domain-specific handles; it neither constructs nor hosts them.
  UI crates receive that facade, not database pools, concrete hosted
  implementations, or persistence adapters. `Engine`, `Services`, and the
  engine error types live directly in the crate root; they do not require a
  generic `application` wrapper. `DeferredResolver` is a private root-level
  composition helper named after its type, because it exists only to bridge
  Engine's peer-session/presence startup dependency. Capability-owned stores
  remain crate-private unless an intentional external replacement port is
  introduced.
- A directory must represent a real responsibility shared by several
  substantial concerns; do not introduce wrapper submodules around an
  otherwise independent file or turn every small definition into a leaf
  module. Internal grouping does not require exposing the child module
  publicly: the parent facade remains the canonical API boundary.
- Avoid module inception in the Rust API. When a directory already supplies
  the domain name, prefer a distinct owned role such as `actor/runtime.rs`
  over `actor/actor.rs`. When a responsibility and its primary type
  intentionally share a name, keep the type-matching filename (for example,
  `decoder/decoder.rs`) behind a neutrally named private module and reexport
  the type from the parent facade; never expose `decoder::decoder`.
- A source file represents one cohesive concern, usually centred on a primary
  type or operation rather than on exactly one definition. Keep a type's
  inherent and trait implementations with that type by default. Its directly
  bound aliases, constants, errors, small value types, and private helpers
  should normally remain there too.
- Split a companion into its own same-named `snake_case` file when it is a
  substantial implementation, has an independent lifecycle or responsibility,
  is meaningfully consumed without its former owner, or would otherwise make
  the owner file difficult to navigate. The mere existence of another named
  type or a short implementation is not sufficient reason to split it.
- If an unusually large implementation is split from its type, name the file
  after the responsibility it implements and keep the relationship explicit in
  the parent module. A generic filename such as `session.rs` must not obscure
  that the file is actually an extension of `Provider` or another owner.
- Public service boundaries use explicit names: `ContactsService` for a direct
  capability orchestrator and `ServiceHandle` for the interface to an
  independently hosted service. Hosted implementations use their capability
  name, such as `PeerSessionService`. Private implementation roles may remain
  concise when their module path supplies all missing context.
- A qualifier remains part of a type name when it conveys information not
  supplied by the canonical module path, such as `PeerId`, `LocalShareState`,
  or `EncoderInput` in the `media::codec` facade.
- Direction- or domain-specific files live inside the module that owns them.
  A prefixed file at a parent root is a signal to check its ownership.
- Refactors establish one canonical path. Development-only compatibility
  aliases are not retained.

Tests, fakes, and small test harnesses may group related helper types when that
makes a scenario easier to read.
