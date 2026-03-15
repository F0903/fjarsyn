use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum CaptureMessage {
    CaptureInitialized(
        Result<
            Arc<tokio::sync::RwLock<crate::capture_providers::PlatformCaptureProvider>>,
            Arc<crate::Error>,
        >,
    ),
}
