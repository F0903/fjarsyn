use crate::utils::vector2::Vector2;

pub mod ffmpeg;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum TargetResolution {
    Scale(Vector2),
    Source,
}
