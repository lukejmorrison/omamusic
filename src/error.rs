use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    AuthRequired(String),
    #[error("{0}")]
    Catalog(String),
    #[error("{0}")]
    Playback(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn auth_required(msg: impl Into<String>) -> Self {
        Self::AuthRequired(msg.into())
    }

    pub fn catalog(msg: impl Into<String>) -> Self {
        Self::Catalog(msg.into())
    }

    pub fn playback(msg: impl Into<String>) -> Self {
        Self::Playback(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Auth(_) | Self::AuthRequired(_) => crate::protocol::ERROR_AUTH,
            Self::Catalog(_) => crate::protocol::ERROR_CATALOG,
            Self::Playback(_) => crate::protocol::ERROR_PLAYBACK,
            Self::Invalid(_) | Self::Json(_) => crate::protocol::ERROR_INVALID_REQUEST,
            Self::Io(_) => crate::protocol::ERROR_UNAVAILABLE,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
