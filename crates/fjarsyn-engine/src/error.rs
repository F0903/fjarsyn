use std::fmt;

use crate::service_host::ServiceFailure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStage {
    Database,
    Contacts,
    PeerSessions,
    IdentityPersistence,
    Presence,
    Messaging,
}

impl fmt::Display for StartupStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Database => "database initialization",
            Self::Contacts => "contact loading",
            Self::PeerSessions => "peer-session startup",
            Self::IdentityPersistence => "local-identity persistence",
            Self::Presence => "presence startup",
            Self::Messaging => "messaging startup",
        })
    }
}

#[derive(Debug)]
pub struct StartError {
    stage: StartupStage,
    source: Box<dyn std::error::Error + Send + Sync>,
    rollback: Option<ShutdownError>,
}

impl StartError {
    pub(crate) fn new(
        stage: StartupStage,
        source: impl std::error::Error + Send + Sync + 'static,
        rollback: Option<ShutdownError>,
    ) -> Self {
        Self { stage, source: Box::new(source), rollback }
    }

    pub fn stage(&self) -> StartupStage {
        self.stage
    }

    pub fn rollback_error(&self) -> Option<&ShutdownError> {
        self.rollback.as_ref()
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "engine startup failed during {}: {}", self.stage, self.source)?;
        if let Some(rollback) = self.rollback.as_ref() {
            write!(formatter, "; startup rollback also failed: {rollback}")?;
        }
        Ok(())
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("shutdown failed: {details}")]
pub struct ShutdownError {
    details: String,
    failures: Vec<ServiceFailure>,
    database_close_timed_out: bool,
}

impl ShutdownError {
    pub(crate) fn from_shutdown(
        failures: Vec<ServiceFailure>,
        database_close_timed_out: bool,
    ) -> Option<Self> {
        if failures.is_empty() && !database_close_timed_out {
            return None;
        }
        let mut details = failures.iter().map(ToString::to_string).collect::<Vec<_>>();
        if database_close_timed_out {
            details.push("database: shared shutdown deadline exceeded".into());
        }
        Some(Self { details: details.join("; "), failures, database_close_timed_out })
    }

    pub fn details(&self) -> &str {
        &self.details
    }

    pub fn failures(&self) -> &[ServiceFailure] {
        &self.failures
    }

    pub const fn database_close_timed_out(&self) -> bool {
        self.database_close_timed_out
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, io};

    use super::*;

    #[test]
    fn startup_error_preserves_primary_failure_and_bounded_rollback_diagnostics() {
        let rollback = ShutdownError::from_shutdown(Vec::new(), true).unwrap();
        let error = StartError::new(
            StartupStage::Messaging,
            io::Error::other("primary startup failure"),
            Some(rollback),
        );

        assert_eq!(error.stage(), StartupStage::Messaging);
        assert_eq!(error.source().unwrap().to_string(), "primary startup failure");
        assert!(error.rollback_error().unwrap().database_close_timed_out());
        assert_eq!(
            error.to_string(),
            "engine startup failed during messaging startup: primary startup failure; startup \
             rollback also failed: shutdown failed: database: shared shutdown deadline exceeded"
        );
    }
}
