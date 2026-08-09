# GPU frame resources

This document defines the Windows GPU-frame boundary shared by capture,
hardware decoding, the engine's D3D11-to-D3D12 importer, and desktop frame
presentation. The contract is producer-independent: Windows Graphics Capture
and D3D11VA publish through the same resource type and consumers do not infer
ownership or readiness from a particular source.

## Published frame boundary

A published GPU frame owns one `Arc`-backed immutable `GpuResource`. The
resource contains the exact D3D11 texture exported by its texture NT handle,
the producer's shared ready-fence timeline, and the fence value for that
frame. These values are constructed together and cannot be paired
independently by a caller.

`D3d11FrameWriter` is the only mutable phase. A producer creates a fresh shared
texture, writes or converts into that texture, and calls `finish` exactly once.
`finish` queues the ready-fence signal and transfers the resource into the
immutable published state. The source capture or decode surface is never
substituted for the shared texture named by the frame.

Published textures are not rotating ring-buffer slots. A later frame receives
a different resource, so retaining or rendering an older frame can never race
with a producer overwriting it. Resource lifetime, rather than a reusable-slot
return protocol, determines when the native allocation may be reclaimed.

A GPU frame may additionally retain CPU-readable pixels. That representation
exists for consumers that need software access and for best-effort preview
fallback; it is not required by the GPU ownership or synchronization contract.
When WGC readback is requested, its staging path is independently publishable:
if shared-texture or fence export is unavailable, capture emits a software
frame rather than making successful GPU export a prerequisite for progress.

## Native ownership and identity

Each shared texture NT handle and each producer-timeline fence NT handle has
one RAII owner. Native handles are borrowed only while opening the resource on
the consumer device. Cloning a frame clones its `Arc` ownership; it does not
copy a raw handle or create an independent obligation to close it. Partial
construction, pool reset, and final resource drop therefore all use the same
once-only handle cleanup.

The imported wgpu texture retains the engine resource and the opened D3D12
fence for its complete cached lifetime. The importer also places a marker
submission after its raw queue wait and independently retains those native
owners until that marker completes. A cloned wgpu view can therefore outlive
the import wrapper without outliving producer readiness; after the marker,
the opened D3D12 resource itself owns the shared allocation. Dropping a
capture pool, codec output pool, or original `Frame` cannot invalidate an
import that is still in use.

`GpuResourceId` is the only cache identity. Each immutable resource receives
one opaque process-local ID from a shared monotonic allocator, including across
producer, fence-timeline, and native-device rebuilds. Raw NT handle values are
never identities: Windows may reuse their numeric values after close, and they
convey neither content nor ownership. The ID is not serialized and is not a
security token.

## Producer-consumer synchronization

The readiness sequence is:

1. The D3D11 producer creates the frame's shared texture and records all writes
   to that exact texture.
2. On the same immediate context, it queues an `ID3D11DeviceContext4::Signal`
   on the shared fence with the frame's monotonically increasing ready value,
   flushes the context, and only then publishes the frame.
3. The D3D12 importer opens both the texture and fence, validates the native
   texture descriptor against the frame, and queues
   `ID3D12CommandQueue::Wait` for that ready value.
4. Iced records and submits the texture read on the same D3D12 queue. The queued
   wait orders that read after the D3D11 writes without blocking the UI thread.

The imported object retains the opened texture and fence objects needed by the
queued work. No D3D12-to-D3D11 completion signal is necessary because a
published texture is immutable and is never returned to a producer for reuse.

An import is valid only on the D3D12 backend and a compatible adapter. Format,
dimensions, matching device/queue ownership, texture descriptor, shared-handle
opening, fence opening, and queue-wait failures are typed `ImportError` values
rather than an absent texture or a silently blank draw.

## Desktop presentation and fallback

Format support indicates that a zero-copy import may be attempted; it does not
guarantee that a particular device, adapter, or frame will import successfully.
The desktop source cache is keyed by `GpuResourceId`, while prepared view state
is keyed by both resource identity and its geometry-derived uniforms. Each
prepared view owns its uniform buffer, so two viewers in one render cycle
cannot overwrite one another's placement.

When an import fails, the viewer uploads retained RGBA8 or BGRA8 CPU pixels to
an ordinary wgpu texture when they are available and have the expected layout.
If no supported CPU representation exists, it draws an explicit unavailable
placeholder and reports the typed import failure. The fallback does not imply
always-on CPU readback: hardware paths may intentionally omit CPU pixels when
zero-copy preview is expected.

The desktop cache retains the active sources and prepared views plus, during
the next prepare/draw pass, at most the immediately preceding render-cycle
set. End-of-frame trimming releases every inactive imported wgpu object and
its retained engine resource naturally. It keeps no wall-clock frame history,
does not close raw handles directly, and cannot evict a primitive between
prepare and draw.

Fresh per-frame allocations are the correctness-first implementation of this
contract, not the final throughput optimization. Any future texture pool must
remain bounded and may reuse a slot only after an explicit D3D12 consumer
completion signal proves that presentation has finished with it. Producer
readiness alone is not permission to overwrite a published resource.

## Recovery and verification

Resize and device recovery may rebuild a producer timeline. Frames from the
old producer remain internally coherent for as long as they are retained,
while globally unique resource IDs prevent cache aliasing with its replacement.
WGC resource-pool reuse also requires the exact current D3D11 device identity;
callbacks carrying frames from a superseded device are dropped before they
can touch the replacement pool.
Higher-level codec and device recovery still has to rebuild every dependent
native capability that cannot survive the device change.

Every GPU producer must be verified against the same invariants: it writes the
exact exported texture, publishes only after queuing the ready signal, never
mutates a published resource, and releases all native ownership on final drop.
Windows adapter-backed coverage must additionally exercise D3D11-fence to
D3D12-queue ordering, import and readback of known pixels, retained old frames,
handle lifetime across reset and recovery, and the desktop CPU-upload and
placeholder fallback paths.
