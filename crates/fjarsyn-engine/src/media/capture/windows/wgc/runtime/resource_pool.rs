use std::mem::MaybeUninit;

use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, ID3D11Device,
        ID3D11Texture2D,
    },
    core::Interface,
};

use super::PIPELINE_DEPTH;
use crate::media::{
    capture::windows::{Error, Result},
    frame::GpuImportHandle,
};

#[derive(Debug, Default)]
pub(in crate::media::capture::windows::wgc) struct ResourcePool {
    pub(super) shared_textures: Vec<ID3D11Texture2D>,
    pub(super) shared_handles: Vec<GpuImportHandle>,
    pub(super) staging_textures: Vec<ID3D11Texture2D>,
    pub(super) frame_count: u64,
    pub(super) last_emitted_timestamp_100ns: Option<i64>,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl ResourcePool {
    pub(in crate::media::capture::windows::wgc) fn reset(&mut self) {
        self.shared_textures.clear();
        self.shared_handles.clear();
        self.staging_textures.clear();
        self.width = 0;
        self.height = 0;
        self.frame_count = 0;
        self.last_emitted_timestamp_100ns = None;
    }

    pub(super) fn ensure(
        &mut self,
        device: &ID3D11Device,
        desc: D3D11_TEXTURE2D_DESC,
        require_staging: bool,
    ) -> Result<()> {
        if !self.shared_textures.is_empty()
            && self.width == desc.Width
            && self.height == desc.Height
            && (!require_staging || !self.staging_textures.is_empty())
        {
            return Ok(());
        }

        tracing::info!(
            "Initializing resource pool (Shared: {}, Staging: {}) for size {}x{}",
            true,
            require_staging,
            desc.Width,
            desc.Height
        );

        self.shared_textures.clear();
        self.shared_handles.clear();
        self.staging_textures.clear();
        self.width = desc.Width;
        self.height = desc.Height;
        self.frame_count = 0;
        self.last_emitted_timestamp_100ns = None;

        let mut shared_desc = desc;
        shared_desc.Usage = D3D11_USAGE_DEFAULT;
        shared_desc.BindFlags =
            windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE.0 as u32;
        shared_desc.CPUAccessFlags = 0;
        shared_desc.MiscFlags =
            (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32;

        let mut staging_desc = desc;
        staging_desc.Usage = D3D11_USAGE_STAGING;
        staging_desc.BindFlags = 0;
        staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        staging_desc.MiscFlags = 0;

        for _ in 0..PIPELINE_DEPTH {
            let shared_texture: ID3D11Texture2D = unsafe {
                let mut texture = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                device.CreateTexture2D(&shared_desc, None, Some(texture.as_mut_ptr())).map_err(
                    |error| {
                        tracing::error!(%error, "failed to create a shared capture texture");
                        Error::FailedToCreateTexture(error)
                    },
                )?;
                texture.assume_init().expect("shared capture texture was not returned")
            };

            let shared_handle = unsafe {
                let dxgi_resource: windows::Win32::Graphics::Dxgi::IDXGIResource1 =
                    shared_texture.cast().map_err(|error| {
                        tracing::error!(%error, "failed to access the shared capture texture");
                        error
                    })?;
                dxgi_resource
                    .CreateSharedHandle(
                        None,
                        windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_READ.0
                            | windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_WRITE.0,
                        None,
                    )
                    .map_err(|error| {
                        tracing::error!(%error, "failed to create a shared capture handle");
                        error
                    })?
            };

            self.shared_textures.push(shared_texture);
            self.shared_handles.push(GpuImportHandle::from_windows_nt_handle(shared_handle));

            if require_staging {
                let staging_texture: ID3D11Texture2D = unsafe {
                    let mut texture = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                    device
                        .CreateTexture2D(&staging_desc, None, Some(texture.as_mut_ptr()))
                        .map_err(|error| {
                            tracing::error!(%error, "failed to create a staging capture texture");
                            Error::FailedToCreateTexture(error)
                        })?;
                    texture.assume_init().expect("staging capture texture was not returned")
                };
                self.staging_textures.push(staging_texture);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_size_and_counters() {
        let mut pool = ResourcePool {
            width: 1920,
            height: 1080,
            frame_count: 42,
            last_emitted_timestamp_100ns: Some(100),
            ..Default::default()
        };

        pool.reset();

        assert_eq!(pool.width, 0);
        assert_eq!(pool.height, 0);
        assert_eq!(pool.frame_count, 0);
        assert!(pool.last_emitted_timestamp_100ns.is_none());
    }
}
