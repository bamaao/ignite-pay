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

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::state::RouterState;
use crate::storage::QueuedMessage;
use crate::transport::auth::{AuthUser, TokenRequest, TokenResponse, create_token};
use ignite_pay_core::verify_did_signature;

/// HTTP POST endpoint for receiving DIDComm messages.
/// The body should be a JWE/JWS/plaintext DIDComm message.
pub async fn post_message(
    State(state): State<RouterState>,
    body: Bytes,
) -> impl IntoResponse {
    let text = match String::from_utf8(body.to_vec()) {
        Ok(t) => t,
        Err(e) => {
            error!("Invalid UTF-8 in POST body: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid UTF-8".to_string()).into_response();
        }
    };

    info!("Received HTTP DIDComm message ({} bytes)", text.len());
    info!("HTTP message content: {}", text);

    match crate::protocols::dispatch(&text, &state, None).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            error!("HTTP message dispatch error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── REST API Endpoints ─────────────────────────────────────────────────

/// `GET /v1/auth/challenge` — Get a nonce challenge for DID authentication.
/// Returns a nonce that must be signed with the DID's Ed25519 key.
pub async fn auth_challenge(
    State(state): State<RouterState>,
) -> impl IntoResponse {
    // Prune expired entries periodically
    let now = chrono::Utc::now().timestamp();
    if state.auth_challenges.len() > 10_000 {
        state.auth_challenges.retain(|_, &mut expiry| expiry > now);
    }

    let nonce = uuid::Uuid::new_v4().to_string();
    let expiry = now + 300; // 5 minutes
    state.auth_challenges.insert(nonce.clone(), expiry);

    (
        StatusCode::OK,
        axum::Json(serde_json::json!({ "nonce": nonce })),
    )
        .into_response()
}

/// `POST /v1/auth/token` — Exchange DID signature over a challenge nonce for JWT.
pub async fn auth_token(
    State(state): State<RouterState>,
    axum::Json(req): axum::Json<TokenRequest>,
) -> impl IntoResponse {
    if !req.did.starts_with("did:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid DID format" })),
        )
            .into_response();
    }

    // Verify the nonce was issued by us and hasn't expired
    let nonce = match req.nonce {
        Some(ref n) => n.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": "Missing nonce" })),
            )
                .into_response();
        }
    };

    let now = chrono::Utc::now().timestamp();
    let valid = state
        .auth_challenges
        .remove_if(&nonce, |_, &expiry| expiry > now);

    if valid.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid or expired nonce" })),
        )
            .into_response();
    }

    // Verify Ed25519 signature over the nonce
    if !verify_did_signature(&req.did, &nonce, &req.signature) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Invalid signature" })),
        )
            .into_response();
    }

    let secret = &state.config.router.jwt_secret;

    match create_token(&req.did, secret) {
        Ok(token) => (
            StatusCode::OK,
            axum::Json(TokenResponse {
                token,
                expires_in: 3600,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Response body for a single message.
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub msg_id: String,
    pub jwe_envelope: String,
    pub created_at: i64,
}

/// `GET /v1/sync/messages/{msg_id}` — Pull a single message by ID.
pub async fn get_message(
    State(state): State<RouterState>,
    auth: AuthUser,
    Path(msg_id): Path<String>,
) -> impl IntoResponse {
    let msg = match state.message_store.get_message(&auth.did, &msg_id).await {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({ "error": "Message not found" }))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({ "error": e.to_string() }))).into_response();
        }
    };

    let resp = MessageResponse {
        msg_id: msg.id,
        jwe_envelope: msg.encrypted_envelope,
        created_at: msg.queued_at.timestamp(),
    };

    (StatusCode::OK, axum::Json(resp)).into_response()
}

/// Query parameters for the list endpoint.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

/// Response body for the message list endpoint.
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub messages: Vec<MessageResponse>,
    pub has_more: bool,
}

/// `GET /v1/sync/list` — Batch sync messages.
pub async fn list_messages(
    State(state): State<RouterState>,
    auth: AuthUser,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(100).min(1000);

    let messages = match state
        .message_store
        .list_messages(&auth.did, query.after.as_deref(), limit + 1)
        .await
    {
        Ok(msgs) => msgs,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({ "error": e.to_string() }))).into_response();
        }
    };

    let has_more = messages.len() > limit;
    let messages: Vec<MessageResponse> = messages
        .into_iter()
        .take(limit)
        .map(|msg| MessageResponse {
            msg_id: msg.id,
            jwe_envelope: msg.encrypted_envelope,
            created_at: msg.queued_at.timestamp(),
        })
        .collect();

    (
        StatusCode::OK,
        axum::Json(ListResponse { messages, has_more }),
    )
        .into_response()
}

/// Request body for submitting an encrypted command (downlink).
#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub jwe_envelope: String,
}

/// `POST /v1/agents/{agent_id}/command` — Submit encrypted command to forward to agent via WS.
pub async fn submit_command(
    State(state): State<RouterState>,
    auth: AuthUser,
    Path(agent_id): Path<String>,
    axum::Json(req): axum::Json<CommandRequest>,
) -> impl IntoResponse {
    // Forward the JWE envelope to the agent via WebSocket.
    // The agent_id is the DID of the target MCP/Skill agent.
    if state.sessions.is_online(&agent_id) {
        match state.sessions.send_to(&agent_id, &req.jwe_envelope) {
            Ok(()) => {
                info!("Forwarded command to agent {} via WS", agent_id);
                return StatusCode::OK.into_response();
            }
            Err(e) => {
                error!("Failed to forward command to agent {}: {}", agent_id, e);
                return (
                    StatusCode::BAD_GATEWAY,
                    axum::Json(serde_json::json!({ "error": "Failed to deliver to agent" })),
                )
                    .into_response();
            }
        }
    }

    // Agent offline — queue the message for later delivery.
    let queued = QueuedMessage {
        id: uuid::Uuid::new_v4().to_string(),
        sender_did: auth.did,
        recipient_did: agent_id.clone(),
        encrypted_envelope: req.jwe_envelope,
        queued_at: chrono::Utc::now(),
    };

    if let Err(e) = state.message_store.store_for_user(&agent_id, queued).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    info!("Queued command for offline agent: {}", agent_id);
    StatusCode::ACCEPTED.into_response()
}

/// Request body for registering a device token.
#[derive(Debug, Deserialize)]
pub struct RegisterTokenRequest {
    pub fcm_token: Option<String>,
    /// Push channel preference: "fcm" or "websocket". Defaults to "fcm".
    pub push_channel: Option<String>,
}

/// `POST /v1/devices/register-token` — Register FCM device token and/or push channel preference.
pub async fn register_device_token(
    State(state): State<RouterState>,
    auth: AuthUser,
    axum::Json(req): axum::Json<RegisterTokenRequest>,
) -> impl IntoResponse {
    let channel = req.push_channel.as_deref().unwrap_or("fcm");

    // Store push channel preference
    if let Err(e) = state.device_token_store.set_push_channel(&auth.did, channel).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    // Register FCM token if provided (FCM channel)
    if let Some(ref fcm_token) = req.fcm_token {
        if let Err(e) = state
            .device_token_store
            .register_device_token(&auth.did, fcm_token)
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        info!("Registered FCM token for {} (channel: {})", auth.did, channel);
    } else {
        info!("Registered push channel '{}' for {}", channel, auth.did);
    }

    StatusCode::OK.into_response()
}

/// Request body for binding an agent to a user.
#[derive(Debug, Deserialize)]
pub struct BindAgentRequest {
    pub agent_did: String,
}

/// `POST /v1/agents/bind` — Bind an agent DID to the authenticated user.
pub async fn bind_agent(
    State(state): State<RouterState>,
    auth: AuthUser,
    axum::Json(req): axum::Json<BindAgentRequest>,
) -> impl IntoResponse {
    if !req.agent_did.starts_with("did:") {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "Invalid agent DID format" })),
        )
            .into_response();
    }

    if let Err(e) = state
        .agent_binding_store
        .bind(&req.agent_did, &auth.did)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    info!("Bound agent {} to user {}", req.agent_did, auth.did);
    StatusCode::OK.into_response()
}
