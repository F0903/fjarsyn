#[derive(Debug, Clone)]
/// Wrapper for types that may include things like pointers, but are known to be safe to send across threads.
pub struct UnsafeSendWrapper<T>(pub T);

unsafe impl<T> Send for UnsafeSendWrapper<T> {}
