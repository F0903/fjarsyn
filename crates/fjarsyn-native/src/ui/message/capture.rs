use std::sync::Arc;

use fjarsyn_core::capture_providers::PlatformCaptureProvider;

#[derive(Debug, Clone)]
pub enum CaptureMessage {
    CaptureInitialized(
        Result<Arc<tokio::sync::RwLock<PlatformCaptureProvider>>, Arc<crate::Error>>,
    ),
}
