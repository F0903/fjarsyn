use std::error::Error;

use async_trait::async_trait;

use super::shutdown_context::ShutdownContext;

/// A service whose implementation executes independently of its callers.
///
/// The service host retains the implementation while callers receive its
/// cloneable, domain-specific [`ServiceHandle`](Self::ServiceHandle).
/// Implementations must make [`prepare_shutdown`](Self::prepare_shutdown) and
/// [`cancel`](Self::cancel) idempotent and non-blocking. The shutdown future
/// must also remain cooperative: blocking native work belongs behind an
/// independently interruptible or safely detachable owner boundary.
#[async_trait]
pub trait HostedService: Send + 'static {
    /// Human-readable identity used for lifecycle diagnostics.
    const NAME: &'static str;

    type ServiceHandle: Clone + Send + Sync + 'static;
    type Error: Error + Send + Sync + 'static;

    /// Creates the application-facing capability interface before the concrete
    /// service is moved into its host.
    fn service_handle(&self) -> Self::ServiceHandle;

    /// Synchronously prevents or quiesces new work when a coordinated shutdown
    /// must begin before the service can be awaited.
    fn prepare_shutdown(&mut self, _context: ShutdownContext) {}

    /// Performs graceful, bounded shutdown while the host retains ownership.
    /// If the context contains an absolute deadline, implementations must not
    /// start a fresh relative timeout or await cleanup after that instant.
    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error>;

    /// Performs immediate best-effort cancellation when graceful shutdown can
    /// no longer be awaited.
    fn cancel(&mut self);
}
