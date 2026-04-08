use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum MediatorError {
    #[error("DIDComm message error: {0}")]
    Didcomm(String),

    #[error("DID resolution error: {0}")]
    DidResolution(String),

    #[error("Session not found for DID: {0}")]
    SessionNotFound(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(#[from] std::io::Error),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}

impl IntoResponse for MediatorError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            MediatorError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            MediatorError::SessionNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, message).into_response()
    }
}

pub type Result<T> = std::result::Result<T, MediatorError>;
