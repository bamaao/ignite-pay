use affinidi_messaging_didcomm::Message;
use tracing::{info, warn};

use crate::error::{MediatorError, Result};
use crate::state::AppState;
use crate::storage::QueuedMessage;

/// Handle a `forward` message (DIDComm Routing Protocol 2.0).
///
/// The mediator reads `body.next` to determine the final recipient,
/// then either:
/// - Delivers immediately if the recipient is online, or
/// - Queues for later pickup
///
/// The inner message is NOT decrypted — the mediator only handles routing.
pub async fn handle_forward(msg: &Message, state: &AppState) -> Result<()> {
    let next_did = msg
        .body
        .get("next")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            MediatorError::Protocol("Forward message missing 'next' in body".into())
        })?;

    info!("Forward message for recipient: {}", next_did);

    // Extract the inner encrypted message from attachments
    let inner_msg = extract_inner_forward(msg)?;

    // Look up which session owns this recipient DID via the keylist
    let owner_session = state.keylist_store.resolve_session(next_did).await?;

    // Try to deliver online
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

    // Recipient offline or not registered — queue the message
    let queued = QueuedMessage {
        id: uuid::Uuid::new_v4().to_string(),
        sender_did: msg
            .from
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        recipient_did: next_did.to_string(),
        encrypted_envelope: inner_msg,
        queued_at: chrono::Utc::now(),
    };

    state.message_store.enqueue(next_did, queued).await?;
    info!("Queued message for offline recipient: {}", next_did);

    Ok(())
}

/// Extract the inner encrypted message from a forward message's attachments.
fn extract_inner_forward(msg: &Message) -> Result<String> {
    // First try the structured attachments field
    if let Some(attachments) = &msg.attachments {
        if let Some(first) = attachments.first() {
            if let Some(json) = &first.data.json {
                return serde_json::to_string(json)
                    .map_err(MediatorError::Serialization);
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
                        .map_err(MediatorError::Serialization);
                }
            }
        }
    }

    Err(MediatorError::Protocol(
        "Forward message has no inner message in attachments".into(),
    ))
}
