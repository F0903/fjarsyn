use std::{error::Error, fmt};

type BoxError = Box<dyn Error + Send + Sync>;

/// A structured failure produced while stopping one hosted service.
#[derive(Debug)]
pub struct ServiceFailure {
    service: &'static str,
    source: BoxError,
}

impl ServiceFailure {
    pub(super) fn new(service: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self { service, source: Box::new(source) }
    }

    pub(super) fn deadline_exceeded(service: &'static str) -> Self {
        Self::new(service, ShutdownDeadlineExceeded)
    }

    pub fn service(&self) -> &'static str {
        self.service
    }
}

impl fmt::Display for ServiceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.service, self.source)
    }
}

impl Error for ServiceFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("shared shutdown deadline exceeded")]
struct ShutdownDeadlineExceeded;
