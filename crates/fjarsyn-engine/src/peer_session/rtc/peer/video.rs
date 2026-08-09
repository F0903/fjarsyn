use std::{future::Future, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures::FutureExt;
use webrtc::{
    api::media_engine::MIME_TYPE_H264,
    media::{Sample, io::sample_builder::SampleBuilder},
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::RTCRtpTransceiver,
    track::track_remote::TrackRemote,
};

use super::{
    super::{
        Event,
        share_epoch::{self, PacketDisposition},
    },
    Peer,
};
use crate::peer_session::{
    EncodedVideoSample, Error, ShareEpoch,
    media::{OutboundVideoSample, RemoteVideoSample},
};

const SAMPLE_BUILDER_PACKET_WINDOW: u16 = 4096;

impl Peer {
    pub(in crate::peer_session) fn start_remote_track(
        &mut self,
        track: Arc<TrackRemote>,
        transceiver: Arc<RTCRtpTransceiver>,
    ) -> Result<(), Error> {
        let codec = track.codec();
        claim_remote_video_track(&mut self.remote_video_claimed, &codec.capability.mime_type)?;
        let extension_id = share_epoch::negotiated_id(&track.params())?;
        if self.share_epoch_extension_id != Some(extension_id) {
            return Err(Error::Protocol(
                "remote track used the wrong screen-share epoch RTP extension ID".into(),
            ));
        }
        let remote_video_tx = self.remote_video_tx.clone();
        let protocol_events = self.events.clone();
        let max_delay = self.max_depacket_latency;
        let media_ssrc = track.ssrc();
        self.spawn_child("remote-video receiver", async move {
            let clock_rate = codec.capability.clock_rate;
            let new_builder = || {
                SampleBuilder::new(
                    SAMPLE_BUILDER_PACKET_WINDOW,
                    webrtc::rtp::codecs::h264::H264Packet::default(),
                    clock_rate,
                )
                .with_max_time_delay(max_delay)
            };
            let mut builder = new_builder();
            let mut active_epoch: Option<ShareEpoch> = None;

            loop {
                let (packet, _) = match track.read_rtp().await {
                    Ok(packet) => packet,
                    Err(error) => {
                        protocol_events.dispatch(Event::Error(format!(
                            "remote video RTP stream ended: {error}"
                        )));
                        return;
                    }
                };
                let epoch = match share_epoch::decode(&packet.header, extension_id) {
                    Ok(epoch) => epoch,
                    Err(error) => {
                        protocol_events.dispatch(Event::ProtocolError(error.to_string()));
                        return;
                    }
                };
                match share_epoch::classify_packet(active_epoch, epoch) {
                    PacketDisposition::DropStale => continue,
                    PacketDisposition::Advance => {
                        builder = new_builder();
                        active_epoch = Some(epoch);
                    }
                    PacketDisposition::Continue => {}
                }
                builder.push(packet);
                while let Some(sample) = builder.pop() {
                    let starts_after_discontinuity = media_packets_were_dropped(
                        sample.prev_dropped_packets,
                        sample.prev_padding_packets,
                    );
                    let _ = remote_video_tx.send(RemoteVideoSample {
                        epoch: active_epoch.expect("an accepted RTP packet establishes an epoch"),
                        sample: EncodedVideoSample::received(
                            sample.data,
                            sample.duration,
                            starts_after_discontinuity,
                        ),
                    });
                }
            }
        });

        let pc = Arc::downgrade(&self.pc);
        let operation_timeout = self.operation_timeout;
        self.spawn_child("picture-loss feedback", async move {
            let sender = transceiver.sender().await;
            let parameters = sender.get_parameters().await;
            let sender_ssrc = parameters.encodings.first().map(|value| value.ssrc).unwrap_or(0);
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            loop {
                interval.tick().await;
                let Some(pc) = pc.upgrade() else { break };
                let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                    Box::new(PictureLossIndication { sender_ssrc, media_ssrc });
                match tokio::time::timeout(operation_timeout, pc.write_rtcp(&[pli])).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        // A temporary transport-path failure must not permanently
                        // disable recovery feedback. The Peer-owned task is
                        // aborted during shutdown, and a later tick can use a
                        // repaired ICE path.
                        tracing::debug!(%error, "failed to send periodic picture-loss feedback");
                    }
                    Err(error) => {
                        tracing::debug!(%error, "timed out sending periodic picture-loss feedback");
                    }
                }
            }
        });
        Ok(())
    }

    pub(in crate::peer_session) async fn write_video(
        &self,
        tagged: OutboundVideoSample,
    ) -> Result<(), Error> {
        let extension = share_epoch::outbound(tagged.epoch)?;
        let sample = Sample {
            data: tagged.sample.data,
            duration: tagged.sample.duration,
            ..Default::default()
        };
        tokio::time::timeout(
            self.operation_timeout,
            self.video_track.write_sample_with_extensions(&sample, &[extension]),
        )
        .await
        .map_err(|_| Error::OperationTimeout)?
        .map_err(|error| Error::WebRtc(error.to_string()))
    }

    fn spawn_child(
        &mut self,
        name: &'static str,
        future: impl Future<Output = ()> + Send + 'static,
    ) {
        let events = self.events.clone();
        self.tasks.spawn(async move {
            if AssertUnwindSafe(future).catch_unwind().await.is_err() {
                events.dispatch(Event::Error(format!("WebRTC {name} task panicked")));
            }
        });
    }
}

fn media_packets_were_dropped(dropped_packets: u16, padding_packets: u16) -> bool {
    dropped_packets.saturating_sub(padding_packets) != 0
}

pub(super) fn claim_remote_video_track(
    already_claimed: &mut bool,
    mime_type: &str,
) -> Result<(), Error> {
    if !mime_type.eq_ignore_ascii_case(MIME_TYPE_H264) {
        return Err(Error::Protocol(format!(
            "unexpected remote video codec {mime_type}; H.264 is required"
        )));
    }
    if *already_claimed {
        return Err(Error::Protocol("a second remote video track is not allowed".into()));
    }
    *already_claimed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::media_packets_were_dropped;

    #[test]
    fn only_non_padding_sample_builder_drops_break_media_continuity() {
        assert!(!media_packets_were_dropped(0, 0));
        assert!(!media_packets_were_dropped(3, 3));
        assert!(!media_packets_were_dropped(3, 4));
        assert!(media_packets_were_dropped(4, 3));
    }
}
