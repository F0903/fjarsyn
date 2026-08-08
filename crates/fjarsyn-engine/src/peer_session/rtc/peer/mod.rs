//! One WebRTC peer connection and its owned signaling, channel, and media roles.

use std::time::Duration;

use crate::peer_session::Error;

mod callbacks;
mod data_channels;
mod ice_credentials;
#[path = "peer.rs"]
mod implementation;
mod signaling;
mod video;

async fn rtc_operation<T>(
    timeout: Duration,
    operation: impl std::future::Future<Output = webrtc::error::Result<T>>,
) -> Result<T, Error> {
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| Error::OperationTimeout)?
        .map_err(|error| Error::WebRtc(error.to_string()))
}

pub(in crate::peer_session) use implementation::Peer;
