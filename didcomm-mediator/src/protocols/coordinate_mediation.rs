use affinidi_messaging_didcomm::Message;
use tracing::{info, warn};

use crate::error::{MediatorError, Result};
use crate::state::AppState;

/// Handle a `mediate-request` message.
///
/// The client asks the mediator to start mediating for them.
/// If accepted, we respond with a `mediate-grant`.
pub async fn handle_mediate_request(
    msg: &Message,
    state: &AppState,
    session_did: Option<&str>,
) -> Result<()> {
    let from = msg.from.as_deref().or(session_did).ok_or_else(|| {
        MediatorError::Unauthorized("mediate-request requires a 'from' field".into())
    })?;

    info!("mediate-request from: {}", from);

    // Build the mediate-grant response
    let grant = Message::new(
        super::MEDIATE_GRANT,
        serde_json::json!({
            "mediator_did": state.did_agent.mediator_did(),
        }),
    )
    .from(state.did_agent.mediator_did().to_string())
    .to(vec![from.to_string()])
    .thid(msg.id.clone());

    let grant_json = serde_json::to_string(&grant)
        .map_err(MediatorError::Serialization)?;

    // Send via WebSocket if connected
    if let Some(session) = session_did {
        if state.sessions.is_online(session) {
            state.sessions.send_to(session, &grant_json)?;
            info!("Sent mediate-grant to {}", from);
            return Ok(());
        }
    }

    // Fallback: try sending to the 'from' DID directly
    if state.sessions.send_to(from, &grant_json).is_ok() {
        info!("Sent mediate-grant to {} (via from DID)", from);
    } else {
        warn!("Cannot deliver mediate-grant: {} not connected", from);
    }

    Ok(())
}

/// Handle a `keylist-update` message.
///
/// The client tells the mediator which recipient DIDs to route to them.
/// Each update entry has an action: "add" or "remove".
pub async fn handle_keylist_update(
    msg: &Message,
    state: &AppState,
    session_did: Option<&str>,
) -> Result<()> {
    let from = msg.from.as_deref().or(session_did).ok_or_else(|| {
        MediatorError::Unauthorized("keylist-update requires a 'from' field".into())
    })?;

    info!("keylist-update from: {}", from);

    let updates = msg
        .body
        .get("updates")
        .and_then(|v| v.as_array())
        .ok_or_else(|| MediatorError::Protocol("Missing 'updates' array in body".into()))?;

    let mut results = Vec::new();

    for update in updates {
        let recipient_key = update
            .get("recipient_key")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let action = update
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("add");

        match action {
            "add" => {
                state
                    .keylist_store
                    .add_key(from, recipient_key)
                    .await?;
                results.push(serde_json::json!({
                    "recipient_key": recipient_key,
                    "action": "add",
                    "result": "success"
                }));
                info!("Added keylist entry: {} -> {}", recipient_key, from);
            }
            "remove" => {
                state
                    .keylist_store
                    .remove_key(from, recipient_key)
                    .await?;
                results.push(serde_json::json!({
                    "recipient_key": recipient_key,
                    "action": "remove",
                    "result": "success"
                }));
                info!("Removed keylist entry: {} for {}", recipient_key, from);
            }
            _ => {
                results.push(serde_json::json!({
                    "recipient_key": recipient_key,
                    "action": action,
                    "result": "client_error",
                    "error": format!("Unknown action: {}", action)
                }));
            }
        }
    }

    // Build the keylist-update-response
    let response = Message::new(
        super::KEYLIST_UPDATE_RESPONSE,
        serde_json::json!({ "updated": results }),
    )
    .from(state.did_agent.mediator_did().to_string())
    .to(vec![from.to_string()])
    .thid(msg.id.clone());

    let response_json = serde_json::to_string(&response)
        .map_err(MediatorError::Serialization)?;

    // Send via WebSocket
    if state.sessions.is_online(from) {
        state.sessions.send_to(from, &response_json)?;
    } else {
        warn!("Cannot deliver keylist-update-response: {} not connected", from);
    }

    Ok(())
}
