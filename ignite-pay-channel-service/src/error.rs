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
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ChannelServiceError {
    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("state channel error: {0}")]
    StateChannel(#[from] ignite_pay_state_channel::error::StateChannelError),

    #[error("on-chain error: {0}")]
    OnChain(String),

    #[error("solana RPC error: {0}")]
    SolanaRpc(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("peer unreachable: {0}")]
    PeerUnreachable(String),

    #[error("compliance hold active")]
    ComplianceHold,

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ChannelServiceError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ChannelServiceError::ChannelNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            ChannelServiceError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ChannelServiceError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            ChannelServiceError::ComplianceHold => (StatusCode::FORBIDDEN, self.to_string()),
            ChannelServiceError::StateChannel(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

impl From<sled::Error> for ChannelServiceError {
    fn from(e: sled::Error) -> Self {
        ChannelServiceError::Storage(e.to_string())
    }
}
