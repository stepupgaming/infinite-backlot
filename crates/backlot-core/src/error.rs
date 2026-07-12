//! Error types shared across backlot crates.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("validation failed: {0:?}")]
    Validation(Vec<String>),

    #[error("episode plan is empty or missing required structure")]
    EmptyPlan,

    #[error("unknown action token: {0}")]
    UnknownAction(String),

    #[error("unknown camera intent: {0}")]
    UnknownCameraIntent(String),

    #[error("unknown entity id: {0}")]
    UnknownEntity(String),

    #[error("llm request failed: {0}")]
    Llm(String),

    #[error("replay mismatch: {0}")]
    ReplayMismatch(String),

    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
