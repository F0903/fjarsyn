use std::{mem::MaybeUninit, sync::Arc};

use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, ID3D11Device, ID3D11Texture2D,
};
use windows_core::Interface;

use super::{ResourcePool, Result, WgcCaptureProvider};
use crate::{capture_providers::windows::WindowsCaptureError, media::frame::GpuImportHandle};

impl WgcCaptureProvider {
    pub(super) fn reset_resource_pool(pool_arc: &Arc<std::sync::RwLock<ResourcePool>>) {
        let mut pool = pool_arc.write().unwrap();
        pool.shared_textures.clear();
        pool.shared_handles.clear();
        pool.staging_textures.clear();
        pool.width = 0;
        pool.height = 0;
        pool.frame_count = 0;
        pool.last_emitted_timestamp_100ns = None;
    }

    pub(super) fn ensure_resource_pool<'a>(
        device: &'a ID3D11Device,
        pool_arc: &'a Arc<std::sync::RwLock<ResourcePool>>,
        desc: D3D11_TEXTURE2D_DESC,
        require_staging: bool,
    ) -> Result<std::sync::RwLockWriteGuard<'a, ResourcePool>> {
        let mut pool = pool_arc.write().unwrap();

        if pool.shared_textures.is_empty()
            || pool.width != desc.Width
            || pool.height != desc.Height
            || (require_staging && pool.staging_textures.is_empty())
        {
            tracing::info!(
                "Initializing resource pool (Shared: {}, Staging: {}) for size {}x{}",
                true,
                require_staging,
                desc.Width,
                desc.Height
            );

            pool.shared_textures.clear();
            pool.shared_handles.clear();
            pool.staging_textures.clear();
            pool.width = desc.Width;
            pool.height = desc.Height;
            pool.frame_count = 0;
            pool.last_emitted_timestamp_100ns = None;

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

            for _ in 0..Self::PIPELINE_DEPTH {
                let shared_texture: ID3D11Texture2D = unsafe {
                    let mut texture = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                    device
                        .CreateTexture2D(&shared_desc, None, Some(texture.as_mut_ptr()))
                        .map_err(|err| {
                            tracing::error!("Failed to create shared texture: {}", err);
                            WindowsCaptureError::FailedToCreateTexture(err)
                        })?;
                    texture.assume_init().expect("Failed to create shared texture!")
                };

                let shared_handle = unsafe {
                    let dxgi_resource: windows::Win32::Graphics::Dxgi::IDXGIResource1 =
                        shared_texture.cast().map_err(|e| {
                            tracing::error!("Failed to cast to IDXGIResource1: {}", e);
                            e
                        })?;
                    dxgi_resource
                        .CreateSharedHandle(
                            None,
                            windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_READ.0
                                | windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_WRITE.0,
                            None,
                        )
                        .map_err(|e| {
                            tracing::error!("Failed to create shared NT handle: {}", e);
                            e
                        })?
                };

                pool.shared_textures.push(shared_texture);
                pool.shared_handles.push(GpuImportHandle::from_windows_nt_handle(shared_handle));

                if require_staging {
                    let staging_texture: ID3D11Texture2D = unsafe {
                        let mut texture = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                        device
                            .CreateTexture2D(&staging_desc, None, Some(texture.as_mut_ptr()))
                            .map_err(|err| {
                                tracing::error!("Failed to create staging texture: {}", err);
                                WindowsCaptureError::FailedToCreateTexture(err)
                            })?;
                        texture.assume_init().expect("Failed to create staging texture!")
                    };
                    pool.staging_textures.push(staging_texture);
                }
            }
        }

        Ok(pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_resource_pool_clears_size_and_counters() {
        let pool = Arc::new(std::sync::RwLock::new(ResourcePool {
            width: 1920,
            height: 1080,
            frame_count: 42,
            last_emitted_timestamp_100ns: Some(100),
            ..Default::default()
        }));

        WgcCaptureProvider::reset_resource_pool(&pool);

        let pool = pool.read().unwrap();
        assert_eq!(pool.width, 0);
        assert_eq!(pool.height, 0);
        assert_eq!(pool.frame_count, 0);
        assert!(pool.last_emitted_timestamp_100ns.is_none());
    }
}
