use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Bilibili API error: {0}")]
    Api(#[from] BilibiliError),

    #[error("Audio backend error: {0}")]
    Audio(#[from] AudioError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
pub enum BilibiliError {
    #[error("WBI signing error: {0}")]
    Wbi(String),

    #[error("API request failed: {code} - {message}")]
    ApiResponse { code: i64, message: String },

    #[error("No audio stream found")]
    NoAudioStream,

    #[error("Authentication required: {0}")]
    AuthRequired(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("mpv backend error: {0}")]
    Mpv(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("No track loaded")]
    NoTrack,
}
