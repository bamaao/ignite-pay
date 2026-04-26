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

/// Dispatch an incoming message to the appropriate protocol handler.
///
/// `session_did` is the DID of the currently connected WebSocket client (if any).
/// All messages are plaintext JSON (TLS protects the transport).
pub async fn dispatch(text: &str, state: &RouterState, session_did: Option<&str>) -> Result<()> {
    let msg: Message = serde_json::from_str(text)
        .map_err(|e| RouterError::Didcomm(format!("Invalid message: {}", e)))?;
    route_message(&msg, state, session_did).await
}

/// Route a parsed plaintext Message to the appropriate protocol handler.
/// Validates expiration, message age, and checks for replay.
async fn route_message(msg: &Message, state: &RouterState, session_did: Option<&str>) -> Result<()> {
    debug!("Dispatching message type: {}", msg.typ);

    let now_secs = chrono::Utc::now().timestamp() as u64;
    let max_age = state.config.router.max_message_age_seconds;

    // Expiration check: reject if expires_time is set and has passed
    if let Some(expires) = msg.expires_time {
        if expires < now_secs {
            warn!(
                "Rejected expired message {} (expires_time={}, now={})",
                msg.id, expires, now_secs
            );
            return Err(RouterError::Protocol(format!(
                "Message expired: expires_time {} is in the past",
                expires
            )));
        }
    }

    // Age check: reject if created_time is too old
    if let Some(created) = msg.created_time {
        if created + max_age < now_secs {
            warn!(
                "Rejected stale message {} (created_time={}, age={}s, max={})",
                msg.id,
                created,
                now_secs.saturating_sub(created),
                max_age
            );
            return Err(RouterError::Protocol(format!(
                "Message too old: created {}s ago, max allowed {}s",
                now_secs.saturating_sub(created),
                max_age
            )));
        }
    }

    // Replay protection: reject messages we've already processed
    let msg_id = &msg.id;
    let now = chrono::Utc::now().timestamp();
    let ttl = 300; // 5 minutes

    // Periodically prune expired entries
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
