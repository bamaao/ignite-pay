use affinidi_messaging_didcomm::identity::PrivateIdentity;
use affinidi_messaging_didcomm::{DIDCommAgent, Message, UnpackResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Build a plaintext `mediate-request` message.
pub fn build_mediate_request(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
        json!({}),
    )
    .from(from_did.to_string())
}

/// Build a plaintext `keylist-update` message.
pub fn build_keylist_update(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
        json!({
            "updates": [{
                "recipient_key": format!("{}#key-1", from_did),
                "action": "add"
            }]
        }),
    )
    .from(from_did.to_string())
}

/// Build a WS challenge-response message for authentication.
/// Sent by the client (MCP/Phone) to prove ownership of their DID keys.
/// The client encrypts this with `pack_authcrypt` before sending.
pub fn build_ws_challenge_response(
    from_did: &str,
    to_did: &str,
    nonce: &str,
    did_doc: &Value,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/ws-challenge-response",
        json!({
            "nonce": nonce,
            "did_document": did_doc,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a `peer-did-discovery/1.0/discover` message carrying our DID document.
pub fn build_peer_introduction(from_did: &str, did_doc: &Value) -> Message {
    Message::new(
        "https://didcomm.org/peer-did-discovery/1.0/discover",
        json!({
            "did_document": did_doc
        }),
    )
    .from(from_did.to_string())
}

/// Build a Message Pickup 3.0 `status-request` message.
/// Asks the mediator how many messages are queued for this DID.
pub fn build_status_request(from_did: &str) -> Message {
    Message::new(
        "https://didcomm.org/messagepickup/3.0/status-request",
        json!({}),
    )
    .from(from_did.to_string())
}

/// Build a Message Pickup 3.0 `batch-pickup` message.
/// Requests up to `count` queued messages from the mediator.
pub fn build_batch_pickup(from_did: &str, count: usize) -> Message {
    Message::new(
        "https://didcomm.org/messagepickup/3.0/batch-pickup",
        json!({
            "count": count,
        }),
    )
    .from(from_did.to_string())
}

/// Build a payment authorization request message.
/// Sent from the MCP server to the phone app via the mediator.
pub fn build_authorization_request(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    merchant_did: &str,
    amount: u64,
    description: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-request",
        json!({
            "payment_id": payment_id,
            "merchant_did": merchant_did,
            "amount": amount,
            "description": description,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a payment authorization response message.
/// Sent from the phone app back to the MCP server.
pub fn build_authorization_response(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-response",
        json!({
            "payment_id": payment_id,
            "authorized": authorized,
            "list_action": list_action,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Session key data sent from phone to MCP in the V1.0 authorization response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKeyResponseData {
    /// Base58-encoded ephemeral public key.
    pub session_key_pubkey: String,
    /// Base58-encoded ephemeral secret key (seed) — needed by MCP to sign payments.
    pub session_key_secret_key: String,
    /// On-chain registration transaction signature.
    pub session_key_tx_signature: String,
    /// Unix timestamp when the session expires.
    pub session_expires_at: i64,
    /// Maximum spending limit in lamports.
    pub spending_limit: u64,
    /// Permission scopes (e.g., ["sol:transfer", "spl:transfer"]).
    pub scopes: Vec<String>,
}

/// Build a V1.0 payment authorization response with optional session key data.
/// Extends the base auth response with session key fields when authorized=true.
/// Backward compatible: old fields are always present, new fields added only when session key data is provided.
pub fn build_authorization_response_v1(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
) -> Message {
    build_authorization_response_v1_inner(
        from_did,
        to_did,
        payment_id,
        authorized,
        list_action,
        session_key_data,
        None,
        None,
    )
}

/// Build a V1.1 payment authorization response with optional session key data and list metadata.
pub fn build_authorization_response_v1_1(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
    list_label: Option<&str>,
    list_max_amount: Option<u64>,
) -> Message {
    build_authorization_response_v1_inner(
        from_did,
        to_did,
        payment_id,
        authorized,
        list_action,
        session_key_data,
        list_label,
        list_max_amount,
    )
}

fn build_authorization_response_v1_inner(
    from_did: &str,
    to_did: &str,
    payment_id: &str,
    authorized: bool,
    list_action: &str,
    session_key_data: Option<&SessionKeyResponseData>,
    list_label: Option<&str>,
    list_max_amount: Option<u64>,
) -> Message {
    let mut body = json!({
        "payment_id": payment_id,
        "authorized": authorized,
        "list_action": list_action,
    });

    if let Some(sk) = session_key_data {
        body["session_key_pubkey"] = json!(sk.session_key_pubkey);
        body["session_key_secret_key"] = json!(sk.session_key_secret_key);
        body["session_key_tx_signature"] = json!(sk.session_key_tx_signature);
        body["session_expires_at"] = json!(sk.session_expires_at);
        body["spending_limit"] = json!(sk.spending_limit);
        body["scopes"] = json!(sk.scopes);
    }

    // V1.1: list metadata
    if let Some(label) = list_label {
        body["list_label"] = json!(label);
    }
    if let Some(max_amt) = list_max_amount {
        body["list_max_amount"] = json!(max_amt);
    }

    Message::new(
        "https://didcomm.org/ignite-pay/1.0/payment-auth-response",
        body,
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Build a list-sync notification message.
/// Sent from the MCP server to the phone after updating whitelist/blacklist.
pub fn build_list_sync_notification(
    from_did: &str,
    to_did: &str,
    list_type: &str,
    action: &str,
    entry_did: &str,
    new_cid: &str,
) -> Message {
    Message::new(
        "https://didcomm.org/ignite-pay/1.0/list-sync-notification",
        json!({
            "list_type": list_type,
            "action": action,
            "entry_did": entry_did,
            "new_cid": new_cid,
        }),
    )
    .from(from_did.to_string())
    .to(vec![to_did.to_string()])
}

/// Encrypt a DIDComm message using authcrypt (authenticated encryption).
/// Returns a JWE JSON string.
pub fn pack_encrypted(
    agent: &DIDCommAgent,
    msg: &Message,
    sender_did: &str,
    recipient_did: &str,
) -> Result<String, String> {
    agent
        .pack_authcrypt(msg, sender_did, recipient_did)
        .map_err(|e| format!("pack_authcrypt failed: {:?}", e))
}

/// Unpack a JWE (or plaintext) message. Returns the inner Message.
pub fn unpack_message(
    agent: &DIDCommAgent,
    jwe: &str,
    sender_did: Option<&str>,
) -> Result<Message, String> {
    let result = agent
        .unpack(jwe, sender_did)
        .map_err(|e| format!("unpack failed: {:?}", e))?;

    match result {
        UnpackResult::Encrypted { message, .. } => Ok(message),
        UnpackResult::Signed { message, .. } => Ok(message),
        UnpackResult::Plaintext(message) => Ok(message),
    }
}

/// Check if a raw JSON string looks like a JWE (has ciphertext + recipients).
pub fn is_jwe(text: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        v.get("ciphertext").is_some() && v.get("recipients").is_some()
    } else {
        false
    }
}

/// Create a DIDComm agent and register our identity (consumes identity).
/// Returns (agent, did_string).
pub fn create_agent(identity: PrivateIdentity) -> (DIDCommAgent, String) {
    let did = identity.did.clone();
    let mut agent = DIDCommAgent::new();
    agent.add_identity(identity);
    (agent, did)
}
