use tokio::sync::{oneshot, watch};

use super::{
    super::{OwnedPipeline, PIPELINE_SHUTDOWN_TIMEOUT},
    ShareBinding,
};
use crate::peer_session::RemoteVideoSource;

pub(super) struct Pipeline {
    pub(super) binding: ShareBinding,
    pub(super) worker: OwnedPipeline,
    pub(super) source_return: oneshot::Receiver<RemoteVideoSource>,
}

impl Pipeline {
    pub(super) async fn shutdown(mut self) -> Option<RemoteVideoSource> {
        self.worker.request_stop();
        let (_, source) = tokio::join!(
            self.worker.shutdown(),
            tokio::time::timeout(PIPELINE_SHUTDOWN_TIMEOUT, self.source_return)
        );
        match source {
            Ok(Ok(source)) => Some(source),
            Ok(Err(_)) => None,
            Err(_) => {
                tracing::warn!("remote video source was not returned before the shutdown deadline");
                None
            }
        }
    }

    pub(super) async fn park_until_replacement(cancel: &mut watch::Receiver<bool>) {
        let _ = cancel.changed().await;
    }
}
