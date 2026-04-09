use affinidi_messaging_didcomm::identity::PrivateIdentity;
use affinidi_messaging_didcomm::{DIDCommAgent, Message, UnpackResult};
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

/// Encrypt a DIDComm message using authcrypt (authenticated encryption).
#[allow(dead_code)]
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
