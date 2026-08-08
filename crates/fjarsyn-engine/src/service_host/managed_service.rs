use async_trait::async_trait;

use super::{
    hosted_service::HostedService, service_failure::ServiceFailure,
    shutdown_context::ShutdownContext,
};

#[async_trait]
pub(super) trait ErasedHostedService: Send {
    fn name(&self) -> &'static str;
    fn prepare_shutdown(&mut self, context: ShutdownContext);
    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), ServiceFailure>;
    fn cancel(&mut self);
}

pub(super) struct ManagedService<ServiceType>
where
    ServiceType: HostedService,
{
    service: ServiceType,
    completed: bool,
}

impl<ServiceType> ManagedService<ServiceType>
where
    ServiceType: HostedService,
{
    pub(super) fn new(service: ServiceType) -> Self {
        Self { service, completed: false }
    }
}

#[async_trait]
impl<ServiceType> ErasedHostedService for ManagedService<ServiceType>
where
    ServiceType: HostedService,
{
    fn name(&self) -> &'static str {
        ServiceType::NAME
    }

    fn prepare_shutdown(&mut self, context: ShutdownContext) {
        self.service.prepare_shutdown(context);
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), ServiceFailure> {
        let result = self
            .service
            .shutdown(context)
            .await
            .map_err(|source| ServiceFailure::new(ServiceType::NAME, source));
        if result.is_err() {
            self.service.cancel();
        }
        self.completed = true;
        result
    }

    fn cancel(&mut self) {
        if !self.completed {
            self.service.cancel();
            self.completed = true;
        }
    }
}

impl<ServiceType> Drop for ManagedService<ServiceType>
where
    ServiceType: HostedService,
{
    fn drop(&mut self) {
        if !self.completed {
            self.service.cancel();
        }
    }
}
