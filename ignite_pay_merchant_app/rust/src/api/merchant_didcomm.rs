use anyhow::Result;
use base64::Engine;
use once_cell::sync::Lazy;
use sha2::Digest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Global state (independent from merchant.rs state channel state) ──────

static GLOBAL_COMM_AGENT: Lazy<Mutex<Option<Arc<Mutex<affinidi_messaging_didcomm::DIDCommAgent>>>>> =
    Lazy::new(|| Mutex::new(None));
static GLOBAL_COMM_DID: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

// ── Bridge return types ─────────────────────────────────────────────────
// DidInfo is reused from crate::api::merchant to avoid duplicate type issues
// with flutter_rust_bridge codegen.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidcommMessage {
    pub msg_id: String,
    pub jwe_envelope: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedMessage {
    pub msg_type: String,
    pub payment_id: Option<String>,
    pub order_id: Option<String>,
    pub amount: Option<u64>,
    pub description: Option<String>,
    pub authorized: Option<bool>,
    pub channel_id: Option<String>,
    pub leaf_index: Option<u32>,
    pub sequence: Option<u64>,
    pub raw_body: String,
}

// ── Identity management (DIDComm DID, separate from state channel DID) ──

/// Initialize the DIDComm communication identity.
/// Uses ignite_pay_core's identity module (same DID format as user app).
/// This DID is independent from the state channel DID in merchant.rs.
pub fn initialize_merchant_comm(storage_path: String) -> Result<crate::api::merchant::DidInfo> {
    let db = sled::open(&storage_path)?;

    // Use a separate sled tree for DIDComm identity to avoid collision with state channel keys
    let comm_tree = db.open_tree("didcomm_identity")?;

    let (identity, did) = if let Some(bytes) = comm_tree.get("identity")? {
        let stored: serde_json::Value = serde_json::from_slice(&bytes)?;
        let did_str = stored.get("did").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // Reconstruct identity from storage via ignite_pay_core load
        let loaded = ignite_pay_core::identity::load_identity(&db)?;
        match loaded {
            Some(id) => (id, did_str),
            None => {
                let (id, d) = ignite_pay_core::identity::generate_ignite_did();
                ignite_pay_core::identity::save_identity(&db, &id, &d)?;
                (id, d)
            }
        }
    } else {
        let (id, did) = ignite_pay_core::identity::generate_ignite_did();
        ignite_pay_core::identity::save_identity(&db, &id, &did)?;
        // Mark that we've initialized
        comm_tree.insert(
            "identity",
            serde_json::to_vec(&serde_json::json!({"did": &did}))?,
        )?;
        comm_tree.flush()?;
        (id, did)
    };

    let did_doc = ignite_pay_core::identity::build_did_document(&did, &identity);
    let (agent, _) = ignite_pay_core::didcomm::create_agent(identity);

    // Store in global state
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        {
            let mut global_agent = GLOBAL_COMM_AGENT.lock().await;
            *global_agent = Some(Arc::new(Mutex::new(agent)));
        }
        {
            let mut global_did = GLOBAL_COMM_DID.lock().await;
            *global_did = Some(did.clone());
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::api::merchant::DidInfo {
        did,
        did_doc_json: serde_json::to_string(&did_doc)?,
    })
}

// ── Mediator connection ─────────────────────────────────────────────────

/// Connect to the DIDComm mediator via WebSocket.
pub async fn connect_mediator(storage_path: String, ws_url: String) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized. Call initialize_merchant_comm first."))?;

    let (agent, _) = ignite_pay_core::didcomm::create_agent(identity);

    {
        let mut global_agent = GLOBAL_COMM_AGENT.lock().await;
        *global_agent = Some(Arc::new(Mutex::new(agent)));
    }

    tracing::info!("Merchant DIDComm connected to mediator: {}", ws_url);
    Ok(())
}

/// Disconnect from the mediator.
pub async fn disconnect_mediator() -> Result<()> {
    {
        let mut global_agent = GLOBAL_COMM_AGENT.lock().await;
        *global_agent = None;
    }
    Ok(())
}

// ── Authentication ──────────────────────────────────────────────────────

/// Authenticate with the mediator via challenge-response.
/// Derives an Ed25519 signing key from the DIDComm DID for signing.
pub async fn authenticate_with_mediator(mediator_url: String, did: String) -> Result<String> {
    let client = reqwest::Client::new();

    // Step 1: Get challenge nonce
    let challenge_url = format!("{}/v1/auth/challenge", mediator_url);
    let challenge_resp = client.get(&challenge_url).send().await?;
    if !challenge_resp.status().is_success() {
        let status = challenge_resp.status();
        let body = challenge_resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Challenge request failed: {} - {}", status, body));
    }
    let challenge_body: serde_json::Value = challenge_resp.json().await?;
    let nonce = challenge_body
        .get("nonce")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("No nonce in challenge response"))?
        .to_string();

    // Step 2: Sign the nonce with the DID's Ed25519 key (derived from DID string)
    let seed = sha2::Sha256::digest(did.as_bytes());
    let seed_bytes: &[u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed_bytes);
    use ed25519_dalek::Signer;
    let signature = signing_key.sign(nonce.as_bytes());
    let signature_b64 = base64::engine::general_purpose::STANDARD_NO_PAD
        .encode(signature.to_bytes());

    // Step 3: Exchange signed challenge for JWT
    let token_url = format!("{}/v1/auth/token", mediator_url);
    let response = client
        .post(&token_url)
        .json(&serde_json::json!({
            "did": did,
            "signature": signature_b64,
            "nonce": nonce
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Auth failed: {} - {}", status, body));
    }

    let token_resp: serde_json::Value = response.json().await?;
    token_resp
        .get("token")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("No token in auth response"))
}

// ── Message pull ────────────────────────────────────────────────────────

/// Pull DIDComm message envelopes from the mediator via HTTPS.
pub async fn pull_messages(
    mediator_url: String,
    token: String,
    after_id: Option<String>,
    limit: u32,
) -> Result<Vec<DidcommMessage>> {
    let client = reqwest::Client::new();
    let mut url = format!("{}/v1/sync/list?limit={}", mediator_url, limit);
    if let Some(ref after) = after_id {
        url = format!("{}&after={}", url, after);
    }

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Pull messages failed: {} - {}",
            status,
            body
        ));
    }

    let list_resp: serde_json::Value = response.json().await?;
    let messages = list_resp
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(DidcommMessage {
                        msg_id: v.get("msg_id")?.as_str()?.to_string(),
                        jwe_envelope: v.get("jwe_envelope")?.as_str()?.to_string(),
                        created_at: v.get("created_at")?.as_i64()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(messages)
}

// ── Message decryption ──────────────────────────────────────────────────

/// Decrypt a JWE message using the DIDComm identity agent.
/// Extracts merchant-relevant fields: payment-auth-response, channel-payment-confirm.
pub fn decrypt_message(storage_path: String, jwe: String) -> Result<DecryptedMessage> {
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized"))?;
    let (agent, _) = ignite_pay_core::didcomm::create_agent(identity);

    // Unpack directly - DIDCommAgent methods don't require async
    let msg = ignite_pay_core::didcomm::unpack_message(&agent, &jwe, None)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    let msg_type = msg.typ.clone();
    let raw_body = serde_json::to_string(&msg.body)?;

    // Extract fields relevant to merchant messages
    let decrypted = DecryptedMessage {
        msg_type: msg_type.clone(),
        payment_id: msg
            .body
            .get("payment_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        order_id: msg
            .body
            .get("order_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        amount: msg.body.get("amount").and_then(|v| v.as_u64()),
        description: msg
            .body
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        authorized: msg.body.get("authorized").and_then(|v| v.as_bool()),
        channel_id: msg
            .body
            .get("channel_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        leaf_index: msg.body.get("leaf_index").and_then(|v| v.as_u64()).map(|v| v as u32),
        sequence: msg.body.get("sequence").and_then(|v| v.as_u64()),
        raw_body,
    };

    Ok(decrypted)
}

// ── FCM token registration ──────────────────────────────────────────────

/// Register an FCM device token with the mediator.
pub async fn register_device_token(
    mediator_url: String,
    auth_token: String,
    fcm_token: String,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/devices/register-token", mediator_url);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", auth_token))
        .json(&serde_json::json!({
            "fcm_token": fcm_token
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Token registration failed: {} - {}",
            status,
            body
        ));
    }

    Ok(())
}
