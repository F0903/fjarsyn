use futures::Stream;

use crate::capture_providers::shared::Frame;

#[derive(Debug)]
pub struct WindowsCaptureStream {
    closer: Option<tokio::sync::mpsc::Sender<i64>>,
    channel: tokio::sync::mpsc::Receiver<Frame>,

    frame_arrived_token: i64,
}

impl WindowsCaptureStream {
    pub fn new(
        closer: tokio::sync::mpsc::Sender<i64>,
        channel: tokio::sync::mpsc::Receiver<Frame>,
        frame_arrived_token: i64,
    ) -> Self {
        tracing::debug!("Creating WindowsCaptureStream with token: {}", frame_arrived_token);
        Self { closer: Some(closer), channel, frame_arrived_token }
    }
}

impl Stream for WindowsCaptureStream {
    type Item = Frame;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.channel.poll_recv(cx)
    }
}

impl Drop for WindowsCaptureStream {
    fn drop(&mut self) {
        tracing::debug!("Dropping WindowsCaptureStream (token: {})", self.frame_arrived_token);
        if let Some(closer) = self.closer.take() {
            if let Err(err) = closer.try_send(self.frame_arrived_token) {
                tracing::warn!("Failed to send stream close signal: {}", err);
            }
        }
    }
}
