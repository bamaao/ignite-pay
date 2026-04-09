use affinidi_messaging_didcomm::Message;
use tracing::info;

use crate::error::{MediatorError, Result};
use crate::state::AppState;

/// Handle a `peer-did-discovery/1.0/discover` message.
///
/// The sender provides its DID document in `body.did_document`.
/// We parse the keys and register them in:
/// 1. The ignite registry (for lookup)
/// 2. The DIDCommAgent (for JWE unpacking)
pub async fn handle_peer_introduction(
    msg: &Message,
    state: &AppState,
    session_did: Option<&str>,
) -> Result<()> {
    let from = msg.from.as_deref().or(session_did).ok_or_else(|| {
        MediatorError::Unauthorized("peer-introduction requires a 'from' field".into())
    })?;

    info!("peer-introduction from: {}", from);

    let did_doc = msg.body.get("did_document").ok_or_else(|| {
        MediatorError::Protocol("Missing 'did_document' in peer-introduction body".into())
    })?;

    let resolved = crate::did::ignite_resolver::parse_ignite_did_document(from, did_doc)
        .ok_or_else(|| {
            MediatorError::Protocol(format!(
                "Failed to parse DID document for {}",
                from
            ))
        })?;

    info!(
        "Parsed DID document for {}: key_agreement_kid={}",
        resolved.did, resolved.key_agreement_kid
    );

    // Register in ignite registry
    state.ignite_registry.register_peer(resolved.clone());

    // Register in DIDCommAgent so it can decrypt JWEs from this peer
    {
        let mut agent = state.did_agent.write().await;
        agent.add_peer(resolved);
    }

    info!("Registered peer {} in ignite registry and DIDCommAgent", from);

    Ok(())
}
