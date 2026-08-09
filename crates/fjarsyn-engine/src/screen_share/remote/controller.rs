use std::collections::BTreeMap;

use super::Pipeline;
use crate::{
    media::{
        PixelFormat,
        codec::{DecoderWorkerConfig, ServiceHandle as CodecServiceHandle},
    },
    peer_session::{RemoteVideoSource, SessionId},
    screen_share::{Config, Output, RemoteState, ShareBinding, Update},
};

pub(in crate::screen_share) struct Controller {
    output: Output,
    codecs: CodecServiceHandle,
    restart_required: bool,
    standby: BTreeMap<SessionId, RemoteVideoSource>,
    pipelines: BTreeMap<SessionId, Pipeline>,
}

impl Controller {
    pub(in crate::screen_share) fn new(output: Output, codecs: CodecServiceHandle) -> Self {
        Self {
            output,
            codecs,
            restart_required: false,
            standby: BTreeMap::new(),
            pipelines: BTreeMap::new(),
        }
    }

    pub(in crate::screen_share) async fn start(
        &mut self,
        session_id: SessionId,
        binding: ShareBinding,
        config: Config,
    ) -> Result<(), String> {
        if self.restart_required {
            return Err("the video decoder is unavailable until Fjarsyn restarts".into());
        }
        if self.is_running(session_id, binding) {
            return Ok(());
        }
        if let Some(stale) = self.pipelines.remove(&session_id)
            && let Some(source) = stale.shutdown().await
        {
            self.standby.insert(session_id, source);
        }
        let Some(source) = self.standby.remove(&session_id) else {
            let reason = "remote video standby source is unavailable".to_owned();
            self.emit(Update::RemoteState {
                session_id,
                state: RemoteState::Failed(reason.clone()),
            })
            .await;
            return Err(reason);
        };
        let decoder_config = DecoderWorkerConfig {
            transcoding_type: config.video.transcoding_type,
            output_format: PixelFormat::DEFAULT_CAPTURE,
        };
        let decoder = match self.codecs.open_decoder(decoder_config).await {
            Ok(decoder) => decoder,
            Err(error) => {
                self.standby.insert(session_id, source);
                let reason = error.to_string();
                self.emit(Update::RemoteState {
                    session_id,
                    state: RemoteState::Failed(reason.clone()),
                })
                .await;
                return Err(reason);
            }
        };
        self.emit(Update::RemoteState { session_id, state: RemoteState::Starting }).await;
        self.pipelines.insert(
            session_id,
            Pipeline::spawn(
                session_id,
                binding,
                source,
                decoder,
                decoder_config,
                self.codecs.clone(),
                self.output.clone(),
            ),
        );
        self.emit(Update::RemoteState { session_id, state: RemoteState::Active }).await;
        Ok(())
    }

    pub(in crate::screen_share) fn receiver_ready(&self, session_id: SessionId) -> bool {
        self.restart_required
            || self.standby.contains_key(&session_id)
            || self.pipelines.contains_key(&session_id)
    }

    pub(in crate::screen_share) fn restart_required(&self) -> bool {
        self.restart_required
    }

    pub(in crate::screen_share) fn install_standby(
        &mut self,
        session_id: SessionId,
        source: RemoteVideoSource,
    ) {
        if !self.restart_required && !self.receiver_ready(session_id) {
            self.standby.insert(session_id, source);
        }
    }

    pub(in crate::screen_share) fn is_running(
        &self,
        session_id: SessionId,
        binding: ShareBinding,
    ) -> bool {
        self.pipelines
            .get(&session_id)
            .is_some_and(|pipeline| pipeline.binding == binding && !pipeline.worker.is_finished())
    }

    pub(in crate::screen_share) async fn stop_session(&mut self, session_id: SessionId) {
        self.stop(session_id).await;
        self.standby.remove(&session_id);
    }

    pub(in crate::screen_share) async fn stop(&mut self, session_id: SessionId) {
        if let Some(pipeline) = self.pipelines.remove(&session_id)
            && let Some(source) = pipeline.shutdown().await
        {
            self.standby.insert(session_id, source);
        }
        self.emit(Update::RemoteState { session_id, state: RemoteState::Inactive }).await;
    }

    pub(in crate::screen_share) fn require_restart(&mut self) -> bool {
        if self.restart_required {
            return false;
        }
        self.restart_required = true;
        self.standby.clear();
        let mut pipelines = std::mem::take(&mut self.pipelines);
        for pipeline in pipelines.values_mut() {
            pipeline.worker.request_stop();
        }
        drop(pipelines);
        true
    }

    pub(in crate::screen_share) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> bool {
        let mut pipelines = std::mem::take(&mut self.pipelines);
        for pipeline in pipelines.values_mut() {
            pipeline.worker.request_stop();
        }
        let clean = futures::future::join_all(
            pipelines.values_mut().map(|pipeline| pipeline.worker.shutdown_until(deadline)),
        )
        .await
        .into_iter()
        .all(|clean| clean);
        self.standby.clear();
        clean
    }

    fn cancel_now(&mut self) {
        self.standby.clear();
        self.pipelines.clear();
    }

    async fn emit(&self, update: Update) {
        self.output.publish(update);
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel_now();
    }
}
