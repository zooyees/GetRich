//! Typed error surface for the foundation layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("unknown method: {0}")]
    UnknownMethod(String),
    #[error("invalid argument: {0}")]
    InvalidArg(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(_) => "CONFIG",
            Self::Db(_) => "DB",
            Self::UnknownMethod(_) => "UNKNOWN_METHOD",
            Self::InvalidArg(_) => "INVALID_ARG",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Io(_) => "IO",
            Self::Message(_) => "ERROR",
        }
    }
}
