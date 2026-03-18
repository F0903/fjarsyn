use crate::media::frame::Frame;

pub struct ImportedFrameTexture {
    pub texture: wgpu::Texture,
}

pub fn import_frame_texture(device: &wgpu::Device, frame: &Frame) -> Option<ImportedFrameTexture> {
    #[cfg(target_os = "windows")]
    {
        dx12::import_frame_texture(device, frame)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, frame);
        None
    }
}

#[cfg(target_os = "windows")]
mod dx12;
