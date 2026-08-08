//! Encoded-video capabilities shared by peer-session senders and receivers.

mod encoded_video_sample;
mod encoded_video_sink;
mod remote_video_source;

#[cfg(test)]
mod tests;

pub(crate) use encoded_video_sample::EncodedVideoSample;
pub(crate) use encoded_video_sink::EncodedVideoSink;
pub(in crate::peer_session) use encoded_video_sink::{OutboundVideoSample, encoded_video_channel};
pub(crate) use remote_video_source::{RemoteVideoRead, RemoteVideoSource};
pub(in crate::peer_session) use remote_video_source::{RemoteVideoSample, remote_video_channel};
