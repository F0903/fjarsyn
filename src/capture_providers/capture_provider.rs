use super::CaptureFramerate;

pub trait CaptureProvider {
    type Result<T>;
    type Stream;
    type CaptureItem;

    fn create_stream(&mut self, framerate: CaptureFramerate) -> Self::Result<Self::Stream>;
    fn set_capture_item(&mut self, capture_item: Self::CaptureItem) -> Self::Result<()>;
    fn start_capture(&mut self) -> Self::Result<()>;
    fn stop_capture(&mut self) -> Self::Result<()>;
    fn is_capturing(&self) -> bool;

    fn raw_device_handle(&self) -> Option<*mut std::ffi::c_void> {
        None
    }
}
