use std::io;

#[derive(Debug, thiserror::Error)]
pub enum BendclawError {
    #[error("env error: {0}")]
    Env(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("run error: {0}")]
    Run(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("agent error: {0}")]
    Agent(String),

    #[error("cli error: {0}")]
    Cli(String),
}

pub type Result<T> = std::result::Result<T, BendclawError>;
