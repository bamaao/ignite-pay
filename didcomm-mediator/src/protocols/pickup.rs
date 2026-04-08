use affinidi_messaging_didcomm::Message;
use tracing::{info, warn};

use crate::error::{MediatorError, Result};
use crate::state::AppState;

/// Handle a `status-request` or `live-delivery-request` message.
///
/// Responds with the number of queued messages for the requester.
pub async fn handle_status_request(
    msg: &Message,
    state: &AppState,
    session_did: Option<&str>,
) -> Result<()> {
    let from = msg.from.as_deref().or(session_did).ok_or_else(|| {
        MediatorError::Unauthorized("status-request requires a 'from' field".into())
    })?;

    info!("status-request from: {}", from);

    // Count messages for all DIDs registered to this session
    let keys = state.keylist_store.list_keys(from).await?;
    let mut total_count = 0usize;

    for key in &keys {
        total_count += state.message_store.count(key).await?;
    }

    let status = Message::new(
        super::STATUS,
        serde_json::json!({
            "message_count": total_count,
            "duration": "PT0S",
            "live_delivery": msg.typ == super::LIVE_DELIVERY_REQUEST,
        }),
    )
    .from(state.did_agent.mediator_did().to_string())
    .to(vec![from.to_string()])
    .thid(msg.id.clone());

    let status_json =
        serde_json::to_string(&status).map_err(MediatorError::Serialization)?;

    if state.sessions.is_online(from) {
        state.sessions.send_to(from, &status_json)?;
    } else {
        warn!("Cannot deliver status: {} not connected", from);
    }

    Ok(())
}

/// Handle a `batch-pickup` message.
///
/// Delivers up to `body.count` queued messages to the requester.
pub async fn handle_batch_pickup(
    msg: &Message,
    state: &AppState,
    session_did: Option<&str>,
) -> Result<()> {
    let from = msg.from.as_deref().or(session_did).ok_or_else(|| {
        MediatorError::Unauthorized("batch-pickup requires a 'from' field".into())
    })?;

    let limit = msg
        .body
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    info!("batch-pickup from: {} (limit: {})", from, limit);

    // Collect queued messages across all registered DIDs
    let keys = state.keylist_store.list_keys(from).await?;
    let mut all_messages = Vec::new();

    for key in &keys {
        let msgs = state.message_store.dequeue_batch(key, limit).await?;
        for m in msgs {
            all_messages.push(serde_json::json!({
                "message": m.encrypted_envelope,
            }));
        }
    }

    let batch = Message::new(
        super::BATCH,
        serde_json::json!({
            "messages": all_messages,
        }),
    )
    .from(state.did_agent.mediator_did().to_string())
    .to(vec![from.to_string()])
    .thid(msg.id.clone());

    let batch_json =
        serde_json::to_string(&batch).map_err(MediatorError::Serialization)?;

    if state.sessions.is_online(from) {
        state.sessions.send_to(from, &batch_json)?;
        info!(
            "Delivered batch of {} messages to {}",
            all_messages.len(),
            from
        );
    } else {
        warn!("Cannot deliver batch: {} not connected", from);
    }

    Ok(())
}
