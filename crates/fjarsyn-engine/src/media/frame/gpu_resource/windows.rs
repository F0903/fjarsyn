use std::{
    fmt,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{Arc, Mutex},
};

use windows::{
    Win32::{
        Foundation::{GENERIC_ALL, HANDLE},
        Graphics::{
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED,
                D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                ID3D11Device, ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4,
                ID3D11Fence, ID3D11Texture2D,
            },
            Dxgi::{DXGI_SHARED_RESOURCE_READ, IDXGIResource1},
        },
    },
    core::{Error, Interface, PCWSTR},
};

use super::{
    GPU_FRAME_POOL_CAPACITY, GpuFrameId, GpuResource, GpuTextureId,
    pool::{Lease, Pool},
};

#[derive(Debug)]
pub(crate) struct D3d11FrameProducer {
    device: ID3D11Device,
    timeline: Arc<ProducerTimeline>,
    slots: Pool<PooledTexture>,
}

impl D3d11FrameProducer {
    pub(crate) fn new(device: ID3D11Device) -> windows::core::Result<Self> {
        let device5: ID3D11Device5 = device.cast()?;
        let mut fence = None;
        unsafe { device5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence) }?;
        let fence: ID3D11Fence = fence.ok_or_else(Error::empty)?;
        let shared_handle = NtHandle::new(unsafe {
            fence.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())?
        });
        Ok(Self {
            device,
            timeline: Arc::new(ProducerTimeline {
                fence,
                shared_handle,
                ready_values: ReadyValueSequence::new(),
            }),
            slots: Pool::with_capacity(GPU_FRAME_POOL_CAPACITY),
        })
    }

    /// Reserves a pooled texture without blocking.
    ///
    /// `Ok(None)` means every slot is still retained by a frame owner or an
    /// in-flight GPU consumer. Producers must drop the newest output instead
    /// of allocating beyond the fixed pool bound.
    pub(crate) fn try_begin_frame(
        &mut self,
        desc: D3D11_TEXTURE2D_DESC,
    ) -> windows::core::Result<Option<D3d11FrameWriter>> {
        let desc = normalized_shared_frame_desc(desc);

        let slot = self
            .slots
            .try_acquire(|slot| slot.matches(&desc), || PooledTexture::new(&self.device, desc))?;
        let Some(slot) = slot else {
            return Ok(None);
        };

        Ok(Some(D3d11FrameWriter { slot: Some(slot), timeline: self.timeline.clone() }))
    }
}

fn normalized_shared_frame_desc(mut desc: D3D11_TEXTURE2D_DESC) -> D3D11_TEXTURE2D_DESC {
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
    desc.CPUAccessFlags = 0;
    desc.MiscFlags = (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32;
    desc
}

#[derive(Debug)]
pub(crate) struct D3d11FrameWriter {
    slot: Option<Lease<PooledTexture>>,
    timeline: Arc<ProducerTimeline>,
}

impl D3d11FrameWriter {
    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        &self.slot.as_ref().expect("frame writer slot is present").value().texture
    }

    /// Publishes the resource only after the producer queue has signalled that
    /// every preceding write is complete. The slot remains immutable until all
    /// frame owners and submitted GPU consumers release this publication.
    pub(crate) fn finish(
        mut self,
        context: &ID3D11DeviceContext,
    ) -> windows::core::Result<Arc<GpuResource>> {
        let context4: ID3D11DeviceContext4 = match context.cast() {
            Ok(context) => context,
            Err(error) => {
                self.quarantine_slot();
                return Err(error);
            }
        };
        let ready_value = match self.timeline.signal(&context4, context) {
            Ok(ready_value) => ready_value,
            Err(error) => {
                self.quarantine_slot();
                return Err(error);
            }
        };

        let slot = self.slot.take().expect("frame writer slot is present");

        Ok(Arc::new(GpuResource {
            frame_id: GpuFrameId::next(),
            texture_id: slot.value().id,
            windows: Resource { slot, timeline: self.timeline.clone(), ready_value },
        }))
    }

    fn quarantine_slot(&mut self) {
        self.slot.take().expect("frame writer slot is present").quarantine();
    }

    /// Removes a slot from circulation after a producer write may have failed
    /// partway through. The owning producer must be rebuilt before retrying.
    pub(crate) fn quarantine(mut self) {
        self.quarantine_slot();
    }
}

#[derive(Debug)]
struct ProducerTimeline {
    fence: ID3D11Fence,
    shared_handle: NtHandle,
    ready_values: ReadyValueSequence,
}

impl ProducerTimeline {
    fn signal(
        &self,
        context4: &ID3D11DeviceContext4,
        context: &ID3D11DeviceContext,
    ) -> windows::core::Result<u64> {
        self.ready_values.assign_with(|ready_value| {
            unsafe { context4.Signal(&self.fence, ready_value) }?;
            unsafe { context.Flush() };
            Ok(())
        })
    }
}

#[derive(Debug)]
struct ReadyValueSequence {
    next: Mutex<u64>,
}

impl ReadyValueSequence {
    fn new() -> Self {
        Self { next: Mutex::new(1) }
    }

    /// Assigns a value in the same critical section that queues its signal.
    ///
    /// Writers may finish in any order, but a later value cannot become
    /// visible on the native fence before the operation for an earlier value
    /// has been queued and flushed.
    fn assign_with<E>(&self, assign: impl FnOnce(u64) -> Result<(), E>) -> Result<u64, E> {
        let mut next = self.next.lock().expect("GPU producer fence timeline lock poisoned");
        let ready_value = *next;
        let following = ready_value.checked_add(1).expect("GPU producer fence timeline exhausted");
        // Consume the value before calling native code. A failed Signal has
        // ambiguous GPU-side effects, so reusing its value could let an old
        // attempt satisfy a later frame's wait. Fence-value gaps are safe.
        *next = following;
        assign(ready_value)?;
        Ok(ready_value)
    }
}

#[derive(Debug)]
pub(crate) struct Resource {
    slot: Lease<PooledTexture>,
    timeline: Arc<ProducerTimeline>,
    ready_value: u64,
}

impl Resource {
    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        &self.slot.value().texture
    }

    pub(crate) fn shared_handle(&self) -> HANDLE {
        self.slot.value().shared_handle.raw()
    }

    pub(crate) fn ready_fence_handle(&self) -> HANDLE {
        self.timeline.shared_handle.raw()
    }

    pub(crate) const fn ready_value(&self) -> u64 {
        self.ready_value
    }
}

#[derive(Debug)]
struct PooledTexture {
    id: GpuTextureId,
    desc: D3D11_TEXTURE2D_DESC,
    texture: ID3D11Texture2D,
    shared_handle: NtHandle,
}

impl PooledTexture {
    fn new(device: &ID3D11Device, desc: D3D11_TEXTURE2D_DESC) -> windows::core::Result<Self> {
        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
        let texture = texture.ok_or_else(Error::empty)?;
        let dxgi_resource: IDXGIResource1 = texture.cast()?;
        let shared_handle = NtHandle::new(unsafe {
            dxgi_resource.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0, None)?
        });

        Ok(Self { id: GpuTextureId::next(), desc, texture, shared_handle })
    }

    fn matches(&self, desc: &D3D11_TEXTURE2D_DESC) -> bool {
        texture_descriptors_match(&self.desc, desc)
    }
}

fn texture_descriptors_match(left: &D3D11_TEXTURE2D_DESC, right: &D3D11_TEXTURE2D_DESC) -> bool {
    left.Width == right.Width
        && left.Height == right.Height
        && left.MipLevels == right.MipLevels
        && left.ArraySize == right.ArraySize
        && left.Format == right.Format
        && left.SampleDesc.Count == right.SampleDesc.Count
        && left.SampleDesc.Quality == right.SampleDesc.Quality
        && left.Usage == right.Usage
        && left.BindFlags == right.BindFlags
        && left.CPUAccessFlags == right.CPUAccessFlags
        && left.MiscFlags == right.MiscFlags
}

struct NtHandle(OwnedHandle);

impl NtHandle {
    fn new(handle: HANDLE) -> Self {
        assert!(!handle.is_invalid(), "native API returned an invalid NT handle");
        // SAFETY: Both CreateSharedHandle calls transfer one newly-created NT
        // handle to the caller. OwnedHandle closes that exact handle once.
        Self(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }
}

impl fmt::Debug for NtHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NtHandle(..)")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
    };

    use windows::Win32::Graphics::{
        Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_GDI_COMPATIBLE,
            D3D11_USAGE_STAGING,
        },
        Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
    };

    use super::*;

    #[test]
    fn shared_frame_descriptor_strips_incompatible_source_properties() {
        let source = D3D11_TEXTURE2D_DESC {
            Width: 1920,
            Height: 1080,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_GDI_COMPATIBLE.0 as u32,
        };

        let normalized = normalized_shared_frame_desc(source);

        assert_eq!(normalized.Width, source.Width);
        assert_eq!(normalized.Height, source.Height);
        assert_eq!(normalized.Format, source.Format);
        assert_eq!(normalized.SampleDesc, source.SampleDesc);
        assert_eq!(normalized.Usage, D3D11_USAGE_DEFAULT);
        assert_eq!(
            normalized.BindFlags,
            (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32
        );
        assert_eq!(normalized.CPUAccessFlags, 0);
        assert_eq!(
            normalized.MiscFlags,
            (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32
        );
    }

    #[test]
    fn ready_values_are_ordered_with_the_operations_that_publish_them() {
        let sequence = Arc::new(ReadyValueSequence::new());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first_sequence = sequence.clone();
        let first = thread::spawn(move || {
            first_sequence
                .assign_with(|value| {
                    entered_tx.send(value).unwrap();
                    release_rx.recv().unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap()
        });
        assert_eq!(entered_rx.recv().unwrap(), 1);

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second = thread::spawn(move || {
            sequence
                .assign_with(|value| {
                    second_entered_tx.send(value).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap()
        });
        assert!(second_entered_rx.try_recv().is_err());

        release_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap(), 1);
        assert_eq!(second_entered_rx.recv().unwrap(), 2);
        assert_eq!(second.join().unwrap(), 2);
    }

    #[test]
    fn a_failed_signal_attempt_still_consumes_its_ready_value() {
        let sequence = ReadyValueSequence::new();

        assert_eq!(sequence.assign_with(|_| Err::<(), _>("signal failed")), Err("signal failed"));
        assert_eq!(sequence.assign_with(|_| Ok::<_, ()>(())).unwrap(), 2);
    }
}
