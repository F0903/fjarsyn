# GPU frame resources

This document defines the Windows GPU-frame boundary shared by capture,
hardware decoding, the engine's D3D11-to-D3D12 importer, and desktop frame
presentation. Windows Graphics Capture and D3D11VA publish through the same
resource contract, so consumers do not infer ownership or synchronization
from a particular producer.

## Bounded published-frame boundary

Each producer owns a fixed pool of six reusable shared D3D11 texture slots.
Pool reservation is nonblocking and never publishes or leases a seventh slot.
`D3d11FrameWriter` owns the mutable phase of one reserved slot: the producer
writes or converts into that exact texture, signals its shared ready-fence
timeline, and transfers the slot lease into the published `Frame`.

A pooled texture is immutable for the complete lifetime of its published frame
lease. A later frame may reuse the physical texture only after every retained
`Frame` and every submitted desktop draw using it has completed. Dropping a
writer before publication returns its slot immediately. Dropping the producer
does not wait for outstanding leases; late lease drops simply destroy their
native resources instead of returning them to a dead owner.

When all six slots are occupied, the producer drops the newest GPU output
instead of blocking, growing native memory, or overwriting a retained frame.
WGC may still publish the already-requested CPU staging copy. D3D11VA treats
pool pressure as a dropped presentation frame and does not mistake it for an
unsupported hardware path that should trigger expensive software conversion.

For four-byte preview textures, six retained slots consume about 47.5 MiB at
1920×1080 and 189.8 MiB at 3840×2160 per producer, before driver alignment and
small synchronization objects. Replacing an idle incompatible slot constructs
its successor before releasing the old allocation so failure preserves the
working slot; resize or format replacement may therefore transiently hold a
seventh allocation (about 55.4 MiB or 221.5 MiB respectively). Total use still
scales with the number of active local and remote producers, so hardware
verification must measure real committed-memory and handle counts.

A GPU frame may additionally retain CPU-readable pixels. That representation
supports software consumers and best-effort preview fallback; it is not a
prerequisite for the GPU contract. When WGC readback is requested, its staging
path remains independently publishable if shared-texture export is unavailable
or the GPU pool is temporarily full.

## Native ownership and identity

Each texture and producer-timeline NT handle has one RAII owner. Handles are
borrowed only while opening D3D12 objects and are closed exactly once. Cloning
a frame clones an `Arc` lease; it never copies raw-handle ownership. Partial
construction, pool replacement, device reset, and final drop therefore share
the same cleanup path.

Two opaque process-local identities deliberately describe different things:

- `GpuFrameId` identifies one publication and its pixel content. Every
  successful publication receives a fresh value.
- `GpuTextureId` identifies one physical pooled texture allocation. It remains
  stable when that allocation is safely reused for later frame IDs.

The desktop keys imported native objects and reusable GPU views by
`GpuTextureId`; each draw still receives the exact `GpuFrameId`-bearing frame
lease, while content-specific CPU fallbacks use `GpuFrameId` as their key. Raw
NT handle values are never identities: Windows may reuse their numeric values
after close, and they express neither content nor ownership. Neither ID is
serialized or treated as a security token.

## Producer and consumer synchronization

The forward readiness sequence is:

1. The producer nonblockingly reserves a free texture slot.
2. D3D11 records all writes to that exact texture.
3. On the same protected immediate context, it queues
   `ID3D11DeviceContext4::Signal` with a monotonically increasing fence value,
   flushes, and publishes the frame lease.
4. The desktop opens and validates the shared texture and fence once for the
   stable texture ID.
5. Immediately before each actual draw, the importer queues
   `ID3D12CommandQueue::Wait` for that frame's ready value on the same queue
   Iced will use to submit the texture read.

The reverse completion boundary is ownership, not another shared fence. Every
actual imported draw registers `RenderPass::on_submitted_work_done` on Iced's
exact command buffer and moves an opaque draw guard into that callback. The
guard retains the published frame lease until the D3D12 queue has completed
that submission. Multiple viewers or submissions therefore retain independent
guards, and the slot becomes reusable only after the last CPU owner and GPU
consumer have released it.

The desktop runs a lightweight completion pump only while callbacks are
pending so an otherwise idle renderer cannot leave a full pool permanently
stalled. The pump polls for completion; it never blocks application shutdown
waiting for the GPU. If completion cannot be observed after device loss,
bounded backpressure is safer than early slot reuse.

An import is valid only on the D3D12 backend and a compatible adapter. Format,
dimensions, matching device/queue ownership, texture descriptor, shared-handle
opening, fence opening, and queue-wait failures are typed `ImportError` values
rather than an absent texture or a silently blank draw.

Creating the clonable raw wgpu view is an explicit unsafe boundary: safe code
cannot express the lifetime relationship between that view and a later command
buffer. The desktop adapter contains the single audited call site and pairs
every sampled view with its exact frame wait and completion guard.

## Desktop presentation and fallback

Format support means a zero-copy import may be attempted; it does not guarantee
that a particular backend, adapter, or frame will import successfully. The
desktop retains imported physical textures by `GpuTextureId` with a 32-entry
least-recently-used target. It evicts only under pressure, and imports used by
the current prepare/draw cycle are never evicted merely to meet that target.
The cache does not own a published-frame lease and therefore cannot by itself
delay pool reuse. The exact `Frame` supplied to a draw provides the readiness
value and the completion guard.

Prepared views use texture identity for reusable imports, frame identity for
content-specific CPU fallbacks, and geometry-derived uniforms in both cases.
Each view owns its uniform buffer, so two viewers prepared in one render cycle
cannot overwrite one another's placement. Cached imports may survive several
pool rotations, while per-publication fallback state is discarded after its
render cycle.

When import fails, the viewer uploads retained RGBA8 or BGRA8 CPU pixels to an
ordinary wgpu texture when they are available and have the exact expected
layout. Without supported CPU pixels, it draws an explicit unavailable
placeholder and rate-limits the typed failure report. Hardware paths may omit
CPU pixels when zero-copy preview is expected, so this is a conditional
fallback rather than a promise of always-on readback.

## Recovery and verification

Resize and device recovery may replace a pooled texture or rebuild the entire
producer timeline. New allocations receive new texture IDs, while old frame
and draw leases remain internally coherent until released. WGC pool reuse also
requires the exact current D3D11 device identity; callbacks carrying frames
from a superseded device are rejected before touching the replacement pool.
Higher-level codec recovery must still rebuild dependent native capabilities
that cannot survive a device change.

Every producer must be verified against the same invariants: it writes the
exact exported texture, publishes only after the ready signal, never mutates a
leased slot, drops rather than blocks at capacity, and releases all ownership
on final drop. Windows adapter-backed coverage must additionally exercise
D3D11-fence-to-D3D12-queue ordering, retained old content across several pool
rotations, exact draw-completion release, resize and device recovery, bounded
memory and handle counts at 1080p and 4K, and CPU-upload/placeholder fallback.
