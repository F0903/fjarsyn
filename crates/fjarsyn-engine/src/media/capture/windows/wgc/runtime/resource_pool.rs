use std::mem::MaybeUninit;

use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device,
        ID3D11Texture2D,
    },
    core::{IUnknown, Interface},
};

use super::PIPELINE_DEPTH;
use crate::media::{
    capture::windows::{Error, Result},
    frame::D3d11FrameProducer,
};

#[derive(Debug, Default)]
pub(in crate::media::capture::windows::wgc) struct ResourcePool {
    device: Option<ID3D11Device>,
    pub(super) frame_producer: Option<D3d11FrameProducer>,
    gpu_export_attempted: bool,
    pub(super) staging_textures: Vec<ID3D11Texture2D>,
    pub(super) frame_count: u64,
    pub(super) last_emitted_timestamp_100ns: Option<i64>,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl ResourcePool {
    pub(in crate::media::capture::windows::wgc) fn reset(&mut self) {
        self.device = None;
        self.frame_producer = None;
        self.gpu_export_attempted = false;
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
        if self.device.as_ref().is_some_and(|pooled| same_device(pooled, device).unwrap_or(false))
            && self.gpu_export_attempted
            && self.width == desc.Width
            && self.height == desc.Height
            && (!require_staging || !self.staging_textures.is_empty())
        {
            return Ok(());
        }

        let frame_producer = match D3d11FrameProducer::new(device.clone()) {
            Ok(producer) => Some(producer),
            Err(error) if Error::is_recoverable_device_loss_error(&error) => {
                return Err(error.into());
            }
            Err(error) if require_staging => {
                tracing::warn!(
                    %error,
                    "GPU frame export is unavailable; continuing with requested CPU readback"
                );
                None
            }
            Err(error) => return Err(error.into()),
        };
        let mut staging_textures = Vec::new();

        let mut staging_desc = desc;
        staging_desc.Usage = D3D11_USAGE_STAGING;
        staging_desc.BindFlags = 0;
        staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        staging_desc.MiscFlags = 0;

        for _ in 0..PIPELINE_DEPTH {
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
                staging_textures.push(staging_texture);
            }
        }

        tracing::info!(
            "Initialized capture resources (GPU export: {}, CPU readback: {}) for size {}x{}",
            frame_producer.is_some(),
            require_staging,
            desc.Width,
            desc.Height
        );

        self.device = Some(device.clone());
        self.frame_producer = frame_producer;
        self.gpu_export_attempted = true;
        self.staging_textures = staging_textures;
        self.width = desc.Width;
        self.height = desc.Height;
        self.frame_count = 0;
        self.last_emitted_timestamp_100ns = None;

        Ok(())
    }
}

fn same_device(left: &ID3D11Device, right: &ID3D11Device) -> windows::core::Result<bool> {
    let left: IUnknown = left.cast()?;
    let right: IUnknown = right.cast()?;
    Ok(left.as_raw() == right.as_raw())
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
