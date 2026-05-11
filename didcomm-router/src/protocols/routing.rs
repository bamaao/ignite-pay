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
    let inner_preview = if inner_msg.len() > 200 { &inner_msg[..200] } else { &inner_msg };
    let has_ciphertext = inner_msg.contains("\"ciphertext\"");
    let has_type = inner_msg.contains("\"type\"");
    info!("Forward inner_msg: len={}, has_ciphertext={}, has_type={}, preview={}", inner_msg.len(), has_ciphertext, has_type, inner_preview);

    // Look up which session owns this recipient DID via the keylist.
    // Try exact match first, then prefix match (handles DID without fragment vs keylist with #key-1).
    let owner_session = match state.keylist_store.resolve_session(next_did).await? {
        Some(session) => Some(session),
        None => {
            // Fallback: prefix scan in reverse keylist for DIDs like "did:...#key-1"
            state.keylist_store.resolve_session_prefix(next_did).await?
        }
    };

    info!("Forward resolve: next_did={}, owner_session={:?}", next_did, owner_session);

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

    // Use the keylist keys for storage so status-request can find the message.
    // The keylist stores keys like "did:...#key-1" which is what list_keys() returns.
    let storage_keys = if let Some(ref owner) = owner_session {
        state.keylist_store.list_keys(owner).await?
    } else {
        // No session found — store under the raw next_did as fallback
        let mut keys = std::collections::HashSet::new();
        keys.insert(next_did.to_string());
        keys
    };

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

    info!("Forward offline: storing for {} keys: {:?}", next_did, storage_keys);

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
            // Always store in queue under all keylist keys as fallback
            for key in &storage_keys {
                state.message_store.store_for_user(key, queued.clone()).await?;
            }
        }
        _ => {
            // FCM mode: store for pull + send FCM signal
            for key in &storage_keys {
                state.message_store.store_for_user(key, queued.clone()).await?;
            }
            info!("Queued message for offline recipient: {} (keys: {})", next_did, storage_keys.len());

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
/// Extract the inner value from a `serde_json::Value`.
/// If the value is a string, return the inner string directly (avoids double-quoting).
/// Otherwise, serialize it to a JSON string.
fn value_to_raw_string(v: &serde_json::Value) -> Result<String> {
    match v {
        serde_json::Value::String(s) => Ok(s.clone()),
        other => serde_json::to_string(other).map_err(RouterError::Serialization),
    }
}

fn extract_inner_forward(msg: &Message) -> Result<String> {
    // First try the structured attachments field
    if let Some(attachments) = &msg.attachments {
        if let Some(first) = attachments.first() {
            if let Some(json) = &first.data.json {
                return value_to_raw_string(json);
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
                    return value_to_raw_string(json);
                }
            }
        }
    }

    Err(RouterError::Protocol(
        "Forward message has no inner message in attachments".into(),
    ))
}
