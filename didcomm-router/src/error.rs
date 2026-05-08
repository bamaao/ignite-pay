// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum RouterError {
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

    #[error("Sled database error: {0}")]
    Sled(#[from] sled::Error),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}

impl IntoResponse for RouterError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            RouterError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            RouterError::SessionNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, message).into_response()
    }
}

pub type Result<T> = std::result::Result<T, RouterError>;
