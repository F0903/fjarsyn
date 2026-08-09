use std::fmt;

/// Identifies one independently adapted engine output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    PresenceState,
    SessionState,
    SessionEvents,
    MessagingState,
    MessagingEvents,
    ScreenShareState,
    ScreenShareEvents,
    Adapter,
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PresenceState => "presence state",
            Self::SessionState => "peer-session state",
            Self::SessionEvents => "peer-session events",
            Self::MessagingState => "messaging state",
            Self::MessagingEvents => "messaging events",
            Self::ScreenShareState => "screen-share state",
            Self::ScreenShareEvents => "screen-share events",
            Self::Adapter => "engine adapter",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cause {
    SourceClosed,
    RuntimeChannelClosed,
    UnexpectedExit,
    Panicked(String),
}

/// Durable diagnostic emitted when the engine adapter can no longer update the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct Failure {
    source: Source,
    cause: Cause,
}

impl Failure {
    pub(super) const fn source_closed(source: Source) -> Self {
        Self { source, cause: Cause::SourceClosed }
    }

    pub(super) const fn runtime_channel_closed(source: Source) -> Self {
        Self { source, cause: Cause::RuntimeChannelClosed }
    }

    pub(super) const fn unexpected_exit(source: Source) -> Self {
        Self { source, cause: Cause::UnexpectedExit }
    }

    pub(super) fn panicked(source: Source, message: String) -> Self {
        Self { source, cause: Cause::Panicked(message) }
    }

    #[cfg(test)]
    pub(super) const fn source(&self) -> Source {
        self.source
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.source == Source::Adapter {
            formatter.write_str("engine adapter stopped")?;
        } else {
            write!(formatter, "{} engine feed stopped", self.source)?;
        }
        match &self.cause {
            Cause::SourceClosed => formatter.write_str(" because its engine source closed"),
            Cause::RuntimeChannelClosed => {
                formatter.write_str(" because the desktop event channel closed")
            }
            Cause::UnexpectedExit => formatter.write_str(" unexpectedly"),
            Cause::Panicked(message) => write!(formatter, " after a panic: {message}"),
        }
    }
}
