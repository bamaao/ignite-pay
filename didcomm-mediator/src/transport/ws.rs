use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use tracing::{error, info, warn};

use crate::state::AppState;
use crate::storage::QueuedMessage;

/// Handler for WebSocket upgrade requests at `/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Spawn a task to forward messages from the channel to the WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read messages from the client
    let session_mgr = state.sessions.clone();
    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        // The first message must identify the client (plaintext DIDComm with their DID)
        let mut session_did: Option<String> = None;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };

            // If not yet identified, try to parse as a plaintext message to extract sender DID
            if session_did.is_none() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(from) = value.get("from").and_then(|v| v.as_str()) {
                        session_did = Some(from.to_string());
                        session_mgr.register(from.to_string(), tx.clone());
                        info!("WebSocket session registered: {}", from);
                    }
                }
            }

            // Try protocol dispatch first
            if let Err(e) =
                crate::protocols::dispatch(&text, &recv_state, session_did.as_deref()).await
            {
                // If protocol dispatch failed, check if this is a JWE from a registered session
                // that needs to be routed to a bound user (application-level message routing)
                if session_did.is_some() && is_jwe(&text) {
                    if let Err(route_err) =
                        route_application_message(&text, &recv_state, session_did.as_deref()).await
                    {
                        error!(
                            "Both protocol dispatch and application routing failed: dispatch={}, route={}",
                            e, route_err
                        );
                    }
                } else {
                    error!("Protocol dispatch error: {}", e);
                }
            }
        }

        // Clean up session on disconnect
        if let Some(ref did) = session_did {
            session_mgr.unregister(did);
            info!("WebSocket session unregistered: {}", did);
        }

        session_did
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        },
        _ = (&mut recv_task) => {
            send_task.abort();
        },
    }
}

/// Check if a raw string looks like a JWE envelope.
fn is_jwe(text: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        v.get("ciphertext").is_some() && v.get("recipients").is_some()
    } else {
        false
    }
}

/// Route an application-level JWE from a connected agent to its bound user.
///
/// When a registered MCP/Skill agent sends a JWE that is not a DIDComm protocol
/// message (e.g. a payment-auth-request encrypted for the phone), the mediator
/// should store it for the bound user and send an FCM push notification.
async fn route_application_message(
    jwe: &str,
    state: &AppState,
    sender_did: Option<&str>,
) -> crate::error::Result<()> {
    let sender = sender_did.ok_or_else(|| {
        crate::error::MediatorError::Unauthorized(
            "Cannot route application message without sender DID".into(),
        )
    })?;

    // Look up the user bound to this agent
    let user_did = state
        .agent_binding_store
        .get_user_for_agent(sender)
        .await?
        .ok_or_else(|| {
            crate::error::MediatorError::Protocol(format!(
                "Agent {} has no bound user for message routing",
                sender
            ))
        })?;

    let msg_id = uuid::Uuid::new_v4().to_string();
    let queued = QueuedMessage {
        id: msg_id.clone(),
        sender_did: sender.to_string(),
        recipient_did: user_did.clone(),
        encrypted_envelope: jwe.to_string(),
        queued_at: chrono::Utc::now(),
    };

    // Store the message for the user to pull
    state
        .message_store
        .store_for_user(&user_did, queued)
        .await?;

    info!(
        "Routed application message from agent {} to user {} (msg_id={})",
        sender, user_did, msg_id
    );

    // Send FCM push notification if device token is registered
    if let Ok(Some(device_token)) = state.device_token_store.get_device_token(&user_did).await {
        match state.notification_sender.send_signal(&device_token, &msg_id).await {
            Ok(()) => info!("Sent push notification for routed message {} to user {}", msg_id, user_did),
            Err(e) => warn!("Failed to send push notification for routed message: {}", e),
        }
    }

    Ok(())
}
