use std::{
    fmt,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::Arc,
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

use super::{GpuResource, GpuResourceId};

#[derive(Debug)]
pub(crate) struct D3d11FrameProducer {
    device: ID3D11Device,
    timeline: Arc<ProducerTimeline>,
    next_ready_value: u64,
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
            timeline: Arc::new(ProducerTimeline { fence, shared_handle }),
            next_ready_value: 1,
        })
    }

    pub(crate) fn begin_frame(
        &mut self,
        desc: D3D11_TEXTURE2D_DESC,
    ) -> windows::core::Result<D3d11FrameWriter> {
        let desc = normalized_shared_frame_desc(desc);

        let mut texture = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
        let texture = texture.ok_or_else(Error::empty)?;
        let dxgi_resource: IDXGIResource1 = texture.cast()?;
        let shared_handle = NtHandle::new(unsafe {
            dxgi_resource.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0, None)?
        });

        let ready_value = self.next_ready_value;
        self.next_ready_value =
            self.next_ready_value.checked_add(1).expect("GPU producer fence timeline exhausted");

        Ok(D3d11FrameWriter {
            id: GpuResourceId::next(),
            texture,
            shared_handle,
            timeline: self.timeline.clone(),
            ready_value,
        })
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
    id: GpuResourceId,
    texture: ID3D11Texture2D,
    shared_handle: NtHandle,
    timeline: Arc<ProducerTimeline>,
    ready_value: u64,
}

impl D3d11FrameWriter {
    pub(crate) const fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    /// Publishes the resource only after the producer queue has signalled that
    /// every preceding write is complete. The resource is immutable after this
    /// ownership transition.
    pub(crate) fn finish(
        self,
        context: &ID3D11DeviceContext,
    ) -> windows::core::Result<Arc<GpuResource>> {
        let context4: ID3D11DeviceContext4 = context.cast()?;
        unsafe {
            context4.Signal(&self.timeline.fence, self.ready_value)?;
            context.Flush();
        }

        Ok(Arc::new(GpuResource {
            id: self.id,
            windows: Resource {
                texture: self.texture,
                shared_handle: self.shared_handle,
                timeline: self.timeline,
                ready_value: self.ready_value,
            },
        }))
    }
}

#[derive(Debug)]
struct ProducerTimeline {
    fence: ID3D11Fence,
    shared_handle: NtHandle,
}

#[derive(Debug)]
pub(crate) struct Resource {
    texture: ID3D11Texture2D,
    shared_handle: NtHandle,
    timeline: Arc<ProducerTimeline>,
    ready_value: u64,
}

impl Resource {
    pub(crate) const fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    pub(crate) fn shared_handle(&self) -> HANDLE {
        self.shared_handle.raw()
    }

    pub(crate) fn ready_fence_handle(&self) -> HANDLE {
        self.timeline.shared_handle.raw()
    }

    pub(crate) const fn ready_value(&self) -> u64 {
        self.ready_value
    }
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
}
