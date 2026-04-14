use affinidi_messaging_didcomm::Message;
use tracing::{info, warn};

use crate::error::{Result, RouterError};
use crate::state::RouterState;
use crate::storage::QueuedMessage;

/// Handle a `forward` message (DIDComm Routing Protocol 2.0).
///
/// The router reads `body.next` to determine the final recipient,
/// then either:
/// - Delivers immediately if the recipient is online, or
/// - Queues for later pickup
///
/// For user devices, routing respects the user's `push_channel` preference:
/// - "websocket": tries direct WS delivery
/// - "fcm" (default): stores and sends FCM signal
///
/// The inner message is NOT decrypted — the router only handles routing.
pub async fn handle_forward(msg: &Message, state: &RouterState) -> Result<()> {
    let next_did = msg
        .body
        .get("next")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            RouterError::Protocol("Forward message missing 'next' in body".into())
        })?;

    info!("Forward message for recipient: {}", next_did);

    // Extract the inner encrypted message from attachments
    let inner_msg = extract_inner_forward(msg)?;

    // Look up which session owns this recipient DID via the keylist
    let owner_session = state.keylist_store.resolve_session(next_did).await?;

    // Try to deliver online to a registered session (MCP/Skill agents)
    if let Some(ref owner_did) = owner_session {
        if state.sessions.is_online(owner_did) {
            match state.sessions.send_to(owner_did, &inner_msg) {
                Ok(()) => {
                    info!("Forwarded message to online recipient {} (session: {})", next_did, owner_did);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to send to online session {}: {}", owner_did, e);
                }
            }
        }
    }

    // Recipient offline or not registered as a session — route based on push channel
    let msg_id = uuid::Uuid::new_v4().to_string();
    let queued = QueuedMessage {
        id: msg_id.clone(),
        sender_did: msg
            .from
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        recipient_did: next_did.to_string(),
        encrypted_envelope: inner_msg.clone(),
        queued_at: chrono::Utc::now(),
    };

    // Determine push channel preference for this recipient
    let channel = state
        .device_token_store
        .get_push_channel(next_did)
        .await
        .unwrap_or_else(|_| "fcm".to_string());

    match channel.as_str() {
        "websocket" => {
            // Try direct WS delivery to the user device
            if state.sessions.is_online(next_did) {
                match state.sessions.send_to(next_did, &inner_msg) {
                    Ok(()) => {
                        info!(
                            "WS direct push (forward): message {} to user {}",
                            msg_id, next_did
                        );
                    }
                    Err(e) => {
                        warn!(
                            "WS push failed for user {}, falling back to queue: {}",
                            next_did, e
                        );
                    }
                }
            } else {
                info!(
                    "User {} offline (websocket channel), queuing forwarded message {}",
                    next_did, msg_id
                );
            }
            // Always store in queue as fallback
            state.message_store.store_for_user(next_did, queued).await?;
        }
        _ => {
            // FCM mode: store for pull + send FCM signal
            state.message_store.store_for_user(next_did, queued).await?;
            info!("Queued message for offline recipient: {}", next_did);

            // Send FCM push notification if device token is registered
            if let Ok(Some(device_token)) =
                state.device_token_store.get_device_token(next_did).await
            {
                match state
                    .notification_sender
                    .send_signal(&device_token, &msg_id)
                    .await
                {
                    Ok(()) => {
                        info!("Sent push notification for message {} to {}", msg_id, next_did)
                    }
                    Err(e) => warn!("Failed to send push notification: {}", e),
                }
            }
        }
    }

    Ok(())
}

/// Extract the inner encrypted message from a forward message's attachments.
fn extract_inner_forward(msg: &Message) -> Result<String> {
    // First try the structured attachments field
    if let Some(attachments) = &msg.attachments {
        if let Some(first) = attachments.first() {
            if let Some(json) = &first.data.json {
                return serde_json::to_string(json)
                    .map_err(RouterError::Serialization);
            }
            if let Some(b64) = &first.data.base64 {
                return Ok(b64.clone());
            }
        }
    }

    // Fallback: check extra field (used by some forward implementations)
    if let Some(attachments) = msg.extra.get("attachments") {
        if let Some(arr) = attachments.as_array() {
            if let Some(first) = arr.first() {
                if let Some(json) = first.get("data").and_then(|d| d.get("json")) {
                    return serde_json::to_string(json)
                        .map_err(RouterError::Serialization);
                }
            }
        }
    }

    Err(RouterError::Protocol(
        "Forward message has no inner message in attachments".into(),
    ))
}
