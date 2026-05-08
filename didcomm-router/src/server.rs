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

use axum::routing::{get, post};
use axum::Router;

use crate::state::RouterState;
use crate::transport;

/// Build the axum router with all routes.
pub fn build_router(state: RouterState) -> Router {
    Router::new()
        // DIDComm endpoints
        .route("/ws", get(transport::ws::ws_handler))
        .route("/", post(transport::http::post_message))
        .route("/health", get(health))
        // REST API v1
        .route("/v1/auth/challenge", get(transport::http::auth_challenge))
        .route("/v1/auth/token", post(transport::http::auth_token))
        .route("/v1/sync/messages/{msg_id}", get(transport::http::get_message))
        .route("/v1/sync/list", get(transport::http::list_messages))
        .route("/v1/devices/register-token", post(transport::http::register_device_token))
        .route("/v1/agents/bind", post(transport::http::bind_agent))
        .route("/v1/agents/{agent_id}/command", post(transport::http::submit_command))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
