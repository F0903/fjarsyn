use super::{DecoderBackend, EncoderBackend};
use crate::services::codec_service::{DecoderWorkerConfig, EncoderWorkerConfig};

pub(in crate::services::codec_service) trait CodecBackendFactory:
    Send + Sync + 'static
{
    fn create_encoder(
        &self,
        config: EncoderWorkerConfig,
    ) -> Result<Box<dyn EncoderBackend>, String>;

    fn create_decoder(
        &self,
        config: DecoderWorkerConfig,
    ) -> Result<Box<dyn DecoderBackend>, String>;
}
