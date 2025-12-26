use futures::Stream;

use crate::utils::frame::Frame;

#[derive(Debug)]
pub struct WindowsCaptureStream {
    channel: tokio::sync::mpsc::Receiver<Frame>,
}

impl WindowsCaptureStream {
    pub fn new(channel: tokio::sync::mpsc::Receiver<Frame>) -> Self {
        Self { channel }
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
