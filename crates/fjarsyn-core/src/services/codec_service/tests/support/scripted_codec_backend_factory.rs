use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use super::{BlockingGate, test_frame};
use crate::{
    media::frame::Frame,
    services::codec_service::{
        DecoderWorkerConfig, EncoderWorkerConfig,
        backend::{CodecBackendFactory, DecoderBackend, EncoderBackend},
    },
};

#[derive(Clone)]
pub(in crate::services::codec_service::tests) enum EncoderPlan {
    CreateError,
    CreateBlock(BlockingGate),
    Pass,
    CallError,
    Block(BlockingGate),
    DropBlock(BlockingGate),
}

#[derive(Clone)]
pub(in crate::services::codec_service::tests) enum DecoderPlan {
    Pass,
    Block(BlockingGate),
}

#[derive(Clone)]
pub(in crate::services::codec_service::tests) struct ScriptedCodecBackendFactory {
    encoders: Arc<Mutex<VecDeque<EncoderPlan>>>,
    decoders: Arc<Mutex<VecDeque<DecoderPlan>>>,
}

impl ScriptedCodecBackendFactory {
    pub(in crate::services::codec_service::tests) fn new(
        encoders: Vec<EncoderPlan>,
        decoders: Vec<DecoderPlan>,
    ) -> Self {
        Self {
            encoders: Arc::new(Mutex::new(encoders.into())),
            decoders: Arc::new(Mutex::new(decoders.into())),
        }
    }
}

impl CodecBackendFactory for ScriptedCodecBackendFactory {
    fn create_encoder(
        &self,
        _config: EncoderWorkerConfig,
    ) -> Result<Box<dyn EncoderBackend>, String> {
        match self.encoders.lock().unwrap().pop_front().unwrap_or(EncoderPlan::Pass) {
            EncoderPlan::CreateError => Err("scripted encoder creation failure".into()),
            EncoderPlan::CreateBlock(gate) => {
                gate.block();
                Ok(Box::new(ScriptedEncoder(EncoderPlan::Pass)))
            }
            plan => Ok(Box::new(ScriptedEncoder(plan))),
        }
    }

    fn create_decoder(
        &self,
        _config: DecoderWorkerConfig,
    ) -> Result<Box<dyn DecoderBackend>, String> {
        let plan = self.decoders.lock().unwrap().pop_front().unwrap_or(DecoderPlan::Pass);
        Ok(Box::new(ScriptedDecoder(plan)))
    }
}

struct ScriptedEncoder(EncoderPlan);

impl EncoderBackend for ScriptedEncoder {
    fn encode(&mut self, _frame: &Frame) -> Result<Vec<Vec<u8>>, String> {
        match &self.0 {
            EncoderPlan::Pass => Ok(vec![vec![1, 2, 3]]),
            EncoderPlan::CallError => Err("scripted encoder call failure".into()),
            EncoderPlan::Block(gate) => {
                gate.block();
                Ok(vec![vec![4, 5, 6]])
            }
            EncoderPlan::DropBlock(_) => Ok(vec![vec![1, 2, 3]]),
            EncoderPlan::CreateError | EncoderPlan::CreateBlock(_) => {
                unreachable!("creation plan is handled by factory")
            }
        }
    }
}

impl Drop for ScriptedEncoder {
    fn drop(&mut self) {
        if let EncoderPlan::DropBlock(gate) = &self.0 {
            gate.block();
        }
    }
}

struct ScriptedDecoder(DecoderPlan);

impl DecoderBackend for ScriptedDecoder {
    fn decode(&mut self, _packet: &[u8]) -> Result<Option<Arc<Frame>>, String> {
        match &self.0 {
            DecoderPlan::Pass => Ok(Some(test_frame())),
            DecoderPlan::Block(gate) => {
                gate.block();
                Ok(Some(test_frame()))
            }
        }
    }
}
