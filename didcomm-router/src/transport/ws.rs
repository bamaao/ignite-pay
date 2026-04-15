use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::state::RouterState;
use crate::storage::QueuedMessage;

// WS authentication protocol type URIs
const WS_CHALLENGE: &str = "https://didcomm.org/ignite-pay/1.0/ws-challenge";
const WS_CHALLENGE_RESPONSE: &str = "https://didcomm.org/ignite-pay/1.0/ws-challenge-response";
const WS_AUTH_OK: &str = "https://didcomm.org/ignite-pay/1.0/ws-auth-ok";
const WS_AUTH_FAILED: &str = "https://didcomm.org/ignite-pay/1.0/ws-auth-failed";

/// Handler for WebSocket upgrade requests at `/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<RouterState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Result of a successful WS challenge-response authentication.
struct AuthenticatedClient {
    did: String,
    did_doc: Option<serde_json::Value>,
}

/// Phase 0: Challenge-Response authentication.
///
/// The mediator sends a random nonce challenge. The client must respond with
/// a JWE-encrypted challenge-response containing the nonce and their DID document.
/// Successful JWE decryption proves the client holds the private key for their DID.
async fn authenticate_ws_client(
    tx: &tokio::sync::mpsc::UnboundedSender<Message>,
    ws_receiver: &mut futures::stream::SplitStream<WebSocket>,
    state: &RouterState,
) -> anyhow::Result<AuthenticatedClient> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let mediator_did = state.did_agent.router_did();
    let mediator_doc = state.did_agent.did_doc();

    // Send challenge (plaintext)
    let challenge = serde_json::json!({
        "type": WS_CHALLENGE,
        "id": uuid::Uuid::new_v4().to_string(),
        "from": mediator_did,
        "body": {
            "nonce": nonce,
            "did_document": mediator_doc,
        }
    });
    tx.send(Message::Text(challenge.to_string().into()))?;

    // Wait for response with 10s timeout
    let response_text = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match ws_receiver.next().await {
                Some(Ok(Message::Text(t))) => break Ok::<_, anyhow::Error>(t.to_string()),
                Some(Ok(Message::Close(_))) => {
                    break Err(anyhow::anyhow!("Connection closed during auth"));
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    break Err(anyhow::anyhow!("WS error during auth: {}", e));
                }
                None => break Err(anyhow::anyhow!("Stream ended during auth")),
            }
        }
    })
    .await??;

    // Unpack JWE — this proves the client holds the private key
    let agent = state.did_agent.read().await;
    let unpack_result = agent
        .unpack(&response_text, None)
        .map_err(|e| anyhow::anyhow!("JWE unpack failed: {:?}", e))?;

    let msg = match unpack_result {
        affinidi_messaging_didcomm::UnpackResult::Encrypted { message, .. } => message,
        affinidi_messaging_didcomm::UnpackResult::Signed { message, .. } => message,
        affinidi_messaging_didcomm::UnpackResult::Plaintext(message) => message,
    };
    drop(agent);

    // Verify message type
    if msg.typ != WS_CHALLENGE_RESPONSE {
        return Err(anyhow::anyhow!(
            "Wrong message type during auth: {}",
            msg.typ
        ));
    }

    // Verify nonce matches (prevents replay)
    let resp_nonce = msg
        .body
        .get("nonce")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if resp_nonce != nonce {
        return Err(anyhow::anyhow!("Nonce mismatch"));
    }

    // Extract client DID
    let did = msg
        .from
        .ok_or_else(|| anyhow::anyhow!("Missing 'from' in challenge response"))?;

    // Extract DID document for peer registration
    let did_doc = msg.body.get("did_document").cloned();

    Ok(AuthenticatedClient { did, did_doc })
}

async fn handle_socket(socket: WebSocket, state: RouterState) {
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

    // Phase 0: Challenge-Response Authentication
    let auth_result = authenticate_ws_client(&tx, &mut ws_receiver, &state).await;

    let session_did = match auth_result {
        Ok(client) => {
            // Register the peer in mediator's agent for future JWE
            if let Some(ref doc) = client.did_doc {
                if let Some(resolved) =
                    ignite_pay_core::parse_did_document(&client.did, doc)
                {
                    let mut agent = state.did_agent.write().await;
                    agent.add_peer(resolved);
                }
            }

            // Send auth-ok
            let ok_msg = serde_json::json!({
                "type": WS_AUTH_OK,
                "id": uuid::Uuid::new_v4().to_string(),
                "from": state.did_agent.router_did(),
            });
            let _ = tx.send(Message::Text(ok_msg.to_string().into()));

            // Register authenticated session
            state.sessions.register(client.did.clone(), tx.clone());
            info!("WS authenticated: {}", client.did);

            Some(client.did)
        }
        Err(e) => {
            warn!("WS authentication failed: {}", e);
            let failed = serde_json::json!({
                "type": WS_AUTH_FAILED,
                "id": uuid::Uuid::new_v4().to_string(),
                "body": { "reason": e.to_string() }
            });
            let _ = tx.send(Message::Text(failed.to_string().into()));
            None
        }
    };

    // If auth failed, just wait for send_task to finish (it will send the failure message)
    let session_did = match session_did {
        Some(did) => did,
        None => {
            send_task.await.ok();
            return;
        }
    };

    // Phase 1: Normal message loop (post-authentication)
    let session_mgr = state.sessions.clone();
    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        run_message_loop(ws_receiver, &recv_state, &session_did).await;

        // Clean up session on disconnect
        session_mgr.unregister(&session_did);
        info!("WebSocket session unregistered: {}", session_did);
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

/// Phase 1: Normal message loop after successful authentication.
/// Reads messages from the client and dispatches them to protocol handlers.
async fn run_message_loop(
    mut ws_receiver: futures::stream::SplitStream<WebSocket>,
    state: &RouterState,
    session_did: &str,
) {
    while let Some(Ok(msg)) = ws_receiver.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        // Try protocol dispatch first
        if let Err(e) = crate::protocols::dispatch(&text, state, Some(session_did)).await {
            // If protocol dispatch failed, check if this is a JWE from a registered session
            // that needs to be routed to a bound user (application-level message routing)
            if is_jwe(&text) {
                if let Err(route_err) =
                    route_application_message(&text, state, Some(session_did)).await
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
/// message (e.g. a payment-auth-request encrypted for the phone), the router
/// routes based on the user's push channel preference:
/// - "websocket": tries direct WS delivery, falls back to queue
/// - "fcm" (default): stores and sends FCM signal
async fn route_application_message(
    jwe: &str,
    state: &RouterState,
    sender_did: Option<&str>,
) -> crate::error::Result<()> {
    let sender = sender_did.ok_or_else(|| {
        crate::error::RouterError::Unauthorized(
            "Cannot route application message without sender DID".into(),
        )
    })?;

    // Look up the user bound to this agent
    let user_did = state
        .agent_binding_store
        .get_user_for_agent(sender)
        .await?
        .ok_or_else(|| {
            crate::error::RouterError::Protocol(format!(
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

    // Determine push channel preference
    let channel = state
        .device_token_store
        .get_push_channel(&user_did)
        .await
        .unwrap_or_else(|_| "fcm".to_string());

    match channel.as_str() {
        "websocket" => {
            // Try direct WS delivery to the user
            if state.sessions.is_online(&user_did) {
                match state.sessions.send_to(&user_did, jwe) {
                    Ok(()) => {
                        info!(
                            "WS direct push: agent {} -> user {} (msg_id={})",
                            sender, user_did, msg_id
                        );
                    }
                    Err(e) => {
                        warn!(
                            "WS push failed for user {}, falling back to queue: {}",
                            user_did, e
                        );
                    }
                }
            } else {
                info!(
                    "User {} offline (websocket channel), queuing message {}",
                    user_did, msg_id
                );
            }
            // Always store in queue as fallback
            state.message_store.store_for_user(&user_did, queued).await?;
        }
        _ => {
            // FCM mode: store for pull + send FCM signal
            state
                .message_store
                .store_for_user(&user_did, queued)
                .await?;

            info!(
                "Routed application message from agent {} to user {} (msg_id={})",
                sender, user_did, msg_id
            );

            // Send FCM push notification if device token is registered
            if let Ok(Some(device_token)) =
                state.device_token_store.get_device_token(&user_did).await
            {
                match state
                    .notification_sender
                    .send_signal(&device_token, &msg_id)
                    .await
                {
                    Ok(()) => info!(
                        "Sent push notification for routed message {} to user {}",
                        msg_id, user_did
                    ),
                    Err(e) => warn!("Failed to send push notification for routed message: {}", e),
                }
            }
        }
    }

    Ok(())
}
