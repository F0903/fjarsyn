use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::{RwLock, mpsc};
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    media::io::sample_builder::SampleBuilder,
    peer_connection::{RTCPeerConnection, peer_connection_state::RTCPeerConnectionState},
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{RTCRtpTransceiver, rtp_codec::RTPCodecType},
    track::track_remote::TrackRemote,
};

use super::WebRTC;
use crate::networking::{
    protocol::{SignalingMessage, SignalingType},
    webrtc::WebRTCEvent,
};

impl WebRTC {
    const SAMPLE_BUILDER_PACKET_WINDOW: u16 = 4096;

    pub(super) fn setup_ice_candidate_handler(
        pc: &RTCPeerConnection,
        signaling_tx: Arc<RwLock<Option<mpsc::Sender<SignalingMessage>>>>,
        local_id: String,
    ) {
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let signaling_tx = signaling_tx.clone();
            let local_id = local_id.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate
                    && let Ok(candidate_str) = serde_json::to_string(&candidate.to_json().unwrap())
                {
                    let message = SignalingMessage {
                        from: local_id,
                        sig_type: SignalingType::Candidate,
                        data: candidate_str,
                    };
                    let tx = {
                        let lock = signaling_tx.read().await;
                        lock.clone()
                    };
                    if let Some(tx) = tx {
                        let _ = tx.send(message).await;
                    }
                }
            })
        }));
    }

    pub(super) fn setup_connection_state_handler(
        pc: &RTCPeerConnection,
        event_tx: mpsc::Sender<WebRTCEvent>,
    ) {
        let disconnected_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let event_tx = event_tx.clone();
            let disconnected_sent = disconnected_sent.clone();
            Box::pin(async move {
                match state {
                    RTCPeerConnectionState::Connected => {
                        let _ = event_tx.send(WebRTCEvent::Connected).await;
                    }
                    RTCPeerConnectionState::Disconnected
                    | RTCPeerConnectionState::Failed
                    | RTCPeerConnectionState::Closed => {
                        if !disconnected_sent.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            let _ = event_tx.send(WebRTCEvent::Disconnected).await;
                        }
                    }
                    _ => {}
                }
            })
        }));
    }

    pub(super) fn setup_track_handler(
        pc: &Arc<RTCPeerConnection>,
        packet_sink: mpsc::Sender<Bytes>,
        max_depacket_latency: u16,
        tasks: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    ) {
        let pc_weak = Arc::downgrade(pc);
        pc.on_track(Box::new(move |track, _rtp_receiver, rtp_transceiver| {
            if track.kind() == RTPCodecType::Video {
                let pli_handle =
                    Self::spawn_pli_loop(pc_weak.clone(), rtp_transceiver.clone(), track.ssrc());
                let reader_handle = Self::spawn_video_track_reader(
                    track,
                    packet_sink.clone(),
                    max_depacket_latency,
                );
                tasks.lock().unwrap().extend([pli_handle, reader_handle]);
            }
            Box::pin(async {})
        }));
    }

    fn spawn_pli_loop(
        pc_weak: std::sync::Weak<RTCPeerConnection>,
        rtp_transceiver: Arc<RTCRtpTransceiver>,
        media_ssrc: u32,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let sender = rtp_transceiver.sender().await;
            let params = sender.get_parameters().await;
            let local_ssrc = params.encodings.first().map(|encoding| encoding.ssrc).unwrap_or(0);

            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                if let Some(pc) = pc_weak.upgrade() {
                    let pli: Box<dyn webrtc::rtcp::packet::Packet + Send + Sync> =
                        Box::new(PictureLossIndication { sender_ssrc: local_ssrc, media_ssrc });
                    if pc.write_rtcp(&[pli]).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        })
    }

    fn spawn_video_track_reader(
        track: Arc<TrackRemote>,
        packet_sink: mpsc::Sender<Bytes>,
        max_depacket_latency: u16,
    ) -> tokio::task::JoinHandle<()> {
        let clock_rate = track.codec().capability.clock_rate;
        tokio::spawn(async move {
            let depacketizer = webrtc::rtp::codecs::h264::H264Packet::default();
            let mut sample_builder =
                SampleBuilder::new(Self::SAMPLE_BUILDER_PACKET_WINDOW, depacketizer, clock_rate)
                    .with_max_time_delay(Duration::from_millis(u64::from(max_depacket_latency)));
            while let Ok((rtp, _)) = track.read_rtp().await {
                sample_builder.push(rtp);
                while let Some(sample) = sample_builder.pop() {
                    if packet_sink.send(sample.data).await.is_err() {
                        return;
                    }
                }
            }
        })
    }
}
