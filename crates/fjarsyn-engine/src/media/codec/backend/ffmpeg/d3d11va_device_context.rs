/// Minimal bindgen-equivalent view of FFmpeg's `AVD3D11VADeviceContext`.
///
/// FFmpeg owns the pointed-to COM references. Keeping this layout in one place
/// prevents the encoder and decoder FFI boundaries from drifting apart.
#[repr(C)]
pub(super) struct D3d11vaDeviceContext {
    pub(super) device: *mut std::ffi::c_void,
    pub(super) device_context: *mut std::ffi::c_void,
    pub(super) video_device: *mut std::ffi::c_void,
    pub(super) video_context: *mut std::ffi::c_void,
    pub(super) lock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    pub(super) unlock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    pub(super) lock_ctx: *mut std::ffi::c_void,
}
