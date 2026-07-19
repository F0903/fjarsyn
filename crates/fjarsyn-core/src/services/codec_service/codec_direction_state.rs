use crate::services::codec_service::CodecPoison;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CodecDirectionState {
    #[default]
    Available,
    RestartRequired(CodecPoison),
}
