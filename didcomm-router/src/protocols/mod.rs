pub mod coordinate_mediation;
pub mod pickup;
pub mod routing;

use affinidi_messaging_didcomm::Message;
use tracing::{debug, warn};

use crate::error::{Result, RouterError};
use crate::state::RouterState;

// DIDComm protocol type URIs
pub const MEDIATE_REQUEST: &str = "https://didcomm.org/coordinate-mediation/2.0/mediate-request";
pub const MEDIATE_GRANT: &str = "https://didcomm.org/coordinate-mediation/2.0/mediate-grant";
pub const MEDIATE_DENY: &str = "https://didcomm.org/coordinate-mediation/2.0/mediate-deny";
pub const KEYLIST_UPDATE: &str = "https://didcomm.org/coordinate-mediation/2.0/keylist-update";
pub const KEYLIST_UPDATE_RESPONSE: &str =
    "https://didcomm.org/coordinate-mediation/2.0/keylist-update-response";
pub const FORWARD: &str = "https://didcomm.org/routing/2.0/forward";
pub const STATUS_REQUEST: &str = "https://didcomm.org/messagepickup/3.0/status-request";
pub const STATUS: &str = "https://didcomm.org/messagepickup/3.0/status";
pub const BATCH_PICKUP: &str = "https://didcomm.org/messagepickup/3.0/batch-pickup";
pub const BATCH: &str = "https://didcomm.org/messagepickup/3.0/batch";
pub const LIVE_DELIVERY_REQUEST: &str =
    "https://didcomm.org/messagepickup/3.0/live-delivery-request";

/// Dispatch an incoming DIDComm message to the appropriate protocol handler.
///
/// `session_did` is the DID of the currently connected WebSocket client (if any).
/// The raw `text` may be plaintext, JWE, or JWS.
pub async fn dispatch(text: &str, state: &RouterState, session_did: Option<&str>) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| RouterError::Didcomm(format!("Invalid JSON: {}", e)))?;

    // If encrypted (JWE), decrypt first then route the inner message
    if value.get("ciphertext").is_some() && value.get("recipients").is_some() {
        debug!("Received encrypted JWE, decrypting");
        let agent = state.did_agent.read().await;
        let unpack_result = agent
            .unpack(text, None)
            .map_err(|e| RouterError::Didcomm(format!("Unpack failed: {:?}", e)))?;

        let inner_msg = match unpack_result {
            affinidi_messaging_didcomm::UnpackResult::Encrypted { message, .. } => message,
            affinidi_messaging_didcomm::UnpackResult::Signed { message, .. } => message,
            affinidi_messaging_didcomm::UnpackResult::Plaintext(message) => message,
        };
        drop(agent);

        route_message(&inner_msg, state, session_did).await
    } else {
        let msg: Message = serde_json::from_value(value)
            .map_err(|e| RouterError::Didcomm(format!("Invalid message: {}", e)))?;
        route_message(&msg, state, session_did).await
    }
}

/// Route a parsed plaintext Message to the appropriate protocol handler.
/// Checks for replay by tracking seen message IDs with a TTL.
async fn route_message(msg: &Message, state: &RouterState, session_did: Option<&str>) -> Result<()> {
    debug!("Dispatching message type: {}", msg.typ);

    // Replay protection: reject messages we've already processed
    let msg_id = &msg.id;
    let now = chrono::Utc::now().timestamp();
    let ttl = 300; // 5 minutes

    // Periodically prune expired entries (probabilistic — every ~1000 messages)
    if state.seen_message_ids.len() > 100_000 {
        state.seen_message_ids.retain(|_, &mut expiry| expiry > now);
    }

    if let Some(existing) = state.seen_message_ids.get(msg_id) {
        if *existing > now {
            warn!("Replay detected: message {} already processed", msg_id);
            return Err(RouterError::Protocol(format!(
                "Duplicate message ID: {}", msg_id
            )));
        }
    }

    // Mark this message as seen
    state.seen_message_ids.insert(msg_id.clone(), now + ttl);

    match msg.typ.as_str() {
        MEDIATE_REQUEST => {
            coordinate_mediation::handle_mediate_request(msg, state, session_did).await
        }
        KEYLIST_UPDATE => {
            coordinate_mediation::handle_keylist_update(msg, state, session_did).await
        }
        FORWARD => routing::handle_forward(msg, state).await,
        STATUS_REQUEST | LIVE_DELIVERY_REQUEST => {
            pickup::handle_status_request(msg, state, session_did).await
        }
        BATCH_PICKUP => pickup::handle_batch_pickup(msg, state, session_did).await,
        _ => {
            warn!("Unknown message type: {}", msg.typ);
            Err(RouterError::Protocol(format!(
                "Unknown message type: {}",
                msg.typ
            )))
        }
    }
}
