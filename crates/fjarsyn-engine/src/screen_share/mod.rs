//! Screen-capture, codec, and authenticated peer-session media orchestration.

mod command;
mod config;
mod error;
mod event;
mod local;
mod model;
mod output;
mod owned_pipeline;
mod reconciler;
mod remote;
mod runtime;
mod screen_share_service;
mod selection;
mod service_handle;
mod shares;
mod start_operation;

use std::time::Duration;

use command::Command;
pub use config::Config;
pub use error::Error;
pub use event::{CodecDirection, Event};
use model::LocalShareBinding;
pub use model::{LocalState, RemoteState, ShareBinding};
use output::{Output, Update};
use owned_pipeline::{ChildTaskGuard, OwnedPipeline, task_failure};
use reconciler::Reconciler;
use runtime::Runtime;
pub(crate) use screen_share_service::ScreenShareService;
pub use selection::Selection;
use selection::SelectionKey;
pub use service_handle::ServiceHandle;
pub use shares::{SessionMedia, Shares};
use start_operation::{StartOperation, StartOutcome};

const START_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum time allocated to stopping all capture and codec pipelines.
const PIPELINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

fn retains_media_session(phase: crate::peer_session::Phase) -> bool {
    matches!(
        phase,
        crate::peer_session::Phase::Connected | crate::peer_session::Phase::Reconnecting
    )
}

fn permits_local_share_start(session: &crate::peer_session::SessionState) -> bool {
    session.phase == crate::peer_session::Phase::Connected
        && matches!(session.local_share, crate::peer_session::LocalShareState::Inactive)
}
