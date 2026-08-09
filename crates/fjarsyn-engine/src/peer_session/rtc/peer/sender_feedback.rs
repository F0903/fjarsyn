use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures::FutureExt;
use webrtc::rtcp::{
    packet::Packet,
    payload_feedbacks::{
        full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication,
    },
};

use super::Peer;
use crate::peer_session::media::KeyframeRequests;

const MIN_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_millis(250);

impl Peer {
    pub(super) fn start_sender_feedback(&mut self, requests: KeyframeRequests) {
        let sender = Arc::clone(&self.video_sender);
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let feedback_events = events.clone();
            let task = async move {
                let mut last_request = None;
                loop {
                    let (packets, _) = match sender.read_rtcp().await {
                        Ok(feedback) => feedback,
                        Err(error) => {
                            feedback_events.dispatch(super::super::Event::Error(format!(
                                "outbound video RTCP feedback ended: {error}"
                            )));
                            return;
                        }
                    };
                    let now = tokio::time::Instant::now();
                    if requests_keyframe(&packets)
                        && request_interval_elapsed(
                            last_request,
                            now,
                            MIN_KEYFRAME_REQUEST_INTERVAL,
                        )
                    {
                        requests.request();
                        last_request = Some(now);
                    }
                }
            };
            if AssertUnwindSafe(task).catch_unwind().await.is_err() {
                events.dispatch(super::super::Event::Error(
                    "WebRTC sender-feedback task panicked".into(),
                ));
            }
        });
    }
}

fn requests_keyframe(packets: &[Box<dyn Packet + Send + Sync>]) -> bool {
    packets.iter().any(|packet| {
        packet.as_any().is::<PictureLossIndication>() || packet.as_any().is::<FullIntraRequest>()
    })
}

fn request_interval_elapsed(
    last: Option<tokio::time::Instant>,
    now: tokio::time::Instant,
    minimum: Duration,
) -> bool {
    last.is_none_or(|last| now.duration_since(last) >= minimum)
}

#[cfg(test)]
mod tests {
    use webrtc::rtcp::{
        payload_feedbacks::full_intra_request::FirEntry, receiver_report::ReceiverReport,
    };

    use super::*;

    #[test]
    fn only_pli_and_fir_request_a_keyframe() {
        let ordinary: Vec<Box<dyn Packet + Send + Sync>> =
            vec![Box::new(ReceiverReport::default())];
        assert!(!requests_keyframe(&ordinary));

        let pli: Vec<Box<dyn Packet + Send + Sync>> =
            vec![Box::new(PictureLossIndication::default())];
        assert!(requests_keyframe(&pli));

        let fir: Vec<Box<dyn Packet + Send + Sync>> = vec![Box::new(FullIntraRequest {
            fir: vec![FirEntry { ssrc: 7, sequence_number: 1 }],
            ..Default::default()
        })];
        assert!(requests_keyframe(&fir));
    }

    #[test]
    fn feedback_requests_are_rate_limited() {
        let now = tokio::time::Instant::now();
        assert!(request_interval_elapsed(None, now, MIN_KEYFRAME_REQUEST_INTERVAL));
        assert!(!request_interval_elapsed(
            Some(now),
            now + MIN_KEYFRAME_REQUEST_INTERVAL / 2,
            MIN_KEYFRAME_REQUEST_INTERVAL,
        ));
        assert!(request_interval_elapsed(
            Some(now),
            now + MIN_KEYFRAME_REQUEST_INTERVAL,
            MIN_KEYFRAME_REQUEST_INTERVAL,
        ));
    }
}
