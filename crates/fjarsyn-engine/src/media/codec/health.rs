//! Codec availability, operation classification, and restart-required diagnostics.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Encode,
    Decode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Initialize,
    Encode,
    Decode,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoisonReason {
    DeadlineExceeded,
    WorkerTerminated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poison {
    pub direction: Direction,
    pub operation: Operation,
    pub reason: PoisonReason,
}

impl fmt::Display for Poison {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match self.direction {
            Direction::Encode => "video encoding",
            Direction::Decode => "video decoding",
        };
        let cause = match self.reason {
            PoisonReason::DeadlineExceeded => "a codec operation exceeded its deadline",
            PoisonReason::WorkerTerminated => "the codec worker terminated unexpectedly",
        };
        write!(formatter, "{direction} is unavailable until Fjarsyn restarts: {cause}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DirectionState {
    #[default]
    Available,
    RestartRequired(Poison),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub encode: DirectionState,
    pub decode: DirectionState,
}

impl Snapshot {
    pub(in crate::media::codec) fn direction(&self, direction: Direction) -> &DirectionState {
        match direction {
            Direction::Encode => &self.encode,
            Direction::Decode => &self.decode,
        }
    }
}
