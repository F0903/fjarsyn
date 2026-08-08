use std::{cmp::Ordering, fmt};

use tokio::time::Instant;

use super::{
    hosted_service::HostedService,
    managed_service::{ErasedHostedService, ManagedService},
    service_failure::ServiceFailure,
    shutdown_context::ShutdownContext,
};

/// Registration policy for a hosted service.
#[derive(Debug, Clone, Copy)]
pub struct ServicePolicy<Phase> {
    phase: Phase,
    prepare_early: bool,
}

impl<Phase> ServicePolicy<Phase> {
    pub const fn new(phase: Phase) -> Self {
        Self { phase, prepare_early: false }
    }

    /// Marks a service for synchronous preparation before ordered shutdown
    /// begins. Preparation is repeated immediately before the service's final
    /// shutdown and must therefore be idempotent.
    pub const fn prepare_early(mut self) -> Self {
        self.prepare_early = true;
        self
    }
}

struct Entry<Phase> {
    phase: Phase,
    order: u64,
    prepare_early: bool,
    service: Box<dyn ErasedHostedService>,
}

/// Owns heterogeneous hosted services while preserving typed handles at the
/// point of installation.
///
/// `Phase` is application-defined. Lower phases stop first, with registration
/// order used as a stable tie-breaker. The host never performs dynamic service
/// lookup and therefore cannot become a service locator.
pub struct ServiceHost<Phase>
where
    Phase: Copy + Ord,
{
    entries: Vec<Entry<Phase>>,
    next_order: u64,
}

impl<Phase> ServiceHost<Phase>
where
    Phase: Copy + Ord,
{
    pub const fn new() -> Self {
        Self { entries: Vec::new(), next_order: 0 }
    }

    /// Retains `service` and returns its domain-specific capability handle.
    pub fn install<ServiceType>(
        &mut self,
        service: ServiceType,
        policy: ServicePolicy<Phase>,
    ) -> ServiceType::ServiceHandle
    where
        ServiceType: HostedService,
    {
        let handle = service.service_handle();
        let order = self.next_order;
        self.next_order =
            self.next_order.checked_add(1).expect("service registration order exhausted");
        self.entries.push(Entry {
            phase: policy.phase,
            order,
            prepare_early: policy.prepare_early,
            service: Box::new(ManagedService::new(service)),
        });
        handle
    }

    /// Synchronously prepares only services explicitly marked for early
    /// preparation before ordered shutdown begins.
    pub fn prepare_shutdown(&mut self, context: ShutdownContext) {
        for entry in self.entries.iter_mut().filter(|entry| entry.prepare_early) {
            entry.service.prepare_shutdown(context);
        }
    }

    /// Stops every retained service in phase order and reports every failure.
    /// An absolute context deadline is a hard fence: once exhausted, the host
    /// synchronously cancels the current and all remaining services.
    /// Cancelling this future drops the current and remaining managed entries,
    /// which synchronously invokes their cancellation hooks.
    pub async fn shutdown(&mut self, context: ShutdownContext) -> Vec<ServiceFailure> {
        let mut failures = Vec::new();
        let mut entries = self.take_entries_in_shutdown_order().into_iter();
        while let Some(mut entry) = entries.next() {
            if context.deadline().is_some_and(|deadline| Instant::now() >= deadline) {
                let service = entry.service.name();
                entry.service.cancel();
                for mut remaining in entries {
                    remaining.service.cancel();
                }
                failures.push(ServiceFailure::deadline_exceeded(service));
                break;
            }
            entry.service.prepare_shutdown(context);
            let result = match context.deadline() {
                Some(deadline) => {
                    match tokio::time::timeout_at(deadline, entry.service.shutdown(context)).await {
                        Ok(result) => result,
                        Err(_) => {
                            let service = entry.service.name();
                            entry.service.cancel();
                            for mut remaining in entries {
                                remaining.service.cancel();
                            }
                            failures.push(ServiceFailure::deadline_exceeded(service));
                            break;
                        }
                    }
                }
                None => entry.service.shutdown(context).await,
            };
            if let Err(failure) = result {
                failures.push(failure);
            }
        }
        failures
    }

    /// Immediately cancels and releases every retained service in phase order.
    pub fn cancel(&mut self) {
        for mut entry in self.take_entries_in_shutdown_order() {
            entry.service.cancel();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn take_entries_in_shutdown_order(&mut self) -> Vec<Entry<Phase>> {
        let mut entries = std::mem::take(&mut self.entries);
        entries.sort_by(|left, right| match left.phase.cmp(&right.phase) {
            Ordering::Equal => left.order.cmp(&right.order),
            ordering => ordering,
        });
        entries
    }
}

impl<Phase> Default for ServiceHost<Phase>
where
    Phase: Copy + Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Phase> fmt::Debug for ServiceHost<Phase>
where
    Phase: Copy + Ord,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let services = self.entries.iter().map(|entry| entry.service.name()).collect::<Vec<_>>();
        formatter.debug_struct("ServiceHost").field("services", &services).finish()
    }
}

impl<Phase> Drop for ServiceHost<Phase>
where
    Phase: Copy + Ord,
{
    fn drop(&mut self) {
        self.cancel();
    }
}
