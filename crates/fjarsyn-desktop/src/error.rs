#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("UI error: {0}")]
    Ui(#[from] iced::Error),
    #[error("configuration error: {0}")]
    Config(#[from] fjarsyn_engine::config::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
