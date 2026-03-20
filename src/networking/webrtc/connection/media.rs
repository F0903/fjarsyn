use std::time::Duration;

use webrtc::media::Sample;

use super::WebRTC;
use crate::networking::webrtc::{WebRTCError, webrtc_error::WebRTCResult};

impl WebRTC {
    pub fn get_local_id(&self) -> String {
        self.local_peer_id.clone()
    }

    pub async fn write_sample(&self, data: Vec<u8>, duration: Duration) -> WebRTCResult<()> {
        let sample = Sample { data: data.into(), duration, ..Default::default() };
        let track = {
            let lock = self.video_track.read().await;
            lock.clone()
        };
        track.write_sample(&sample).await.map_err(WebRTCError::WriteRTPError)?;
        Ok(())
    }
}
