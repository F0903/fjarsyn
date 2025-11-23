use crate::capture_providers::shared::Frame;
use futures::Stream;

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
        Self {
            closer: Some(closer),
            channel,
            frame_arrived_token,
        }
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
        if let Some(closer) = self.closer.take() {
            closer.blocking_send(self.frame_arrived_token).ok();
        }
    }
}
