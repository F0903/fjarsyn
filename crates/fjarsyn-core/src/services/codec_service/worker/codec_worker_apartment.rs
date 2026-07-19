//! Platform thread context held for the complete native codec lifetime.

pub(in crate::services::codec_service) struct CodecWorkerApartment;

impl CodecWorkerApartment {
    #[cfg(target_os = "windows")]
    pub(in crate::services::codec_service) fn initialize() -> Result<Self, String> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map(|()| Self)
            .map_err(|error| format!("failed to initialize codec worker COM apartment: {error}"))
    }

    #[cfg(not(target_os = "windows"))]
    pub(in crate::services::codec_service) fn initialize() -> Result<Self, String> {
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for CodecWorkerApartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}
