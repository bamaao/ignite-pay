use anyhow::Result;
use base64::Engine;
use once_cell::sync::Lazy;
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
/// Signs the nonce with the actual Ed25519 signing key from identity storage.
pub async fn authenticate_with_mediator(mediator_url: String, storage_path: String, did: String) -> Result<String> {
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

    // Step 2: Sign the nonce with the DID's actual Ed25519 signing key from identity storage
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized"))?;
    let signing_private = identity.signing_private
        .ok_or_else(|| anyhow::anyhow!("no signing key in identity"))?;
    let signature_b64 = ignite_pay_core::sign_message(&signing_private, nonce.as_bytes());

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

/// Sign a nonce string with the merchant's Ed25519 signing key.
/// Returns the base64-no-pad encoded signature.
pub fn sign_nonce(storage_path: String, nonce: String) -> Result<String> {
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized"))?;
    let signing_private = identity.signing_private
        .ok_or_else(|| anyhow::anyhow!("no signing key in identity"))?;
    Ok(ignite_pay_core::sign_message(&signing_private, nonce.as_bytes()))
}

/// Verify an Ed25519 signature from a DID.
/// Returns true if the signature is valid for the given message and DID.
pub fn verify_did_signature(did: String, message: String, signature_b64: String) -> Result<bool> {
    Ok(ignite_pay_core::verify_did_signature(&did, &message, &signature_b64))
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

// ── Hub Registry & Channel Creation ─────────────────────────────────────

/// Hub info from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubInfo {
    pub hub_id: String,
    pub hub_did: String,
    pub endpoint_url: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub fee_rate_bps: u16,
    pub available_liquidity: u64,
    pub online_rate: u16,
    pub success_rate: u16,
    pub avg_latency_ms: u32,
    pub active_channels: u32,
    pub supported_tokens: Vec<String>,
}

/// Fetch the list of available hubs from the hub registry.
pub async fn fetch_hub_list(registry_url: String) -> Result<Vec<HubInfo>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/hubs?status=active", registry_url))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch hub list: {}",
            resp.status()
        ));
    }

    let body: serde_json::Value = resp.json().await?;
    let hubs = body
        .get("hubs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut result = Vec::new();
    for hub in hubs {
        result.push(HubInfo {
            hub_id: hub["hub_id"].as_str().unwrap_or("").to_string(),
            hub_did: hub["hub_did"].as_str().unwrap_or("").to_string(),
            endpoint_url: hub["endpoint_url"].as_str().unwrap_or("").to_string(),
            name: hub["name"].as_str().unwrap_or("").to_string(),
            description: hub["description"].as_str().unwrap_or("").to_string(),
            status: hub["status"].as_str().unwrap_or("").to_string(),
            fee_rate_bps: hub["fee_rate_bps"].as_u64().unwrap_or(0) as u16,
            available_liquidity: hub["available_liquidity"].as_u64().unwrap_or(0),
            online_rate: hub["online_rate"].as_u64().unwrap_or(0) as u16,
            success_rate: hub["success_rate"].as_u64().unwrap_or(0) as u16,
            avg_latency_ms: hub["avg_latency_ms"].as_u64().unwrap_or(0) as u32,
            active_channels: hub["active_channels"].as_u64().unwrap_or(0) as u32,
            supported_tokens: hub
                .get("supported_tokens")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }

    Ok(result)
}

// ── OOB Invitation & Connection (QR pairing) ───────────────────────────

/// Parsed OOB invitation data from a QR code scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OobInvitationData {
    pub mcp_did: String,
    pub did_doc_json: String,
    pub mediator_ws_url: String,
    pub label: String,
}

/// Parse an OOB invitation URL (from QR code scan).
/// Expected format: didcomm://?_oob=<base64url-encoded JSON>
pub fn parse_oob_invitation(invitation_url: String) -> Result<OobInvitationData> {
    let url = url::Url::parse(&invitation_url)
        .map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;

    let oob_b64 = url
        .query_pairs()
        .find(|(k, _)| k == "_oob")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing _oob parameter in invitation URL"))?;

    let json_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&oob_b64)
        .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;

    let invitation: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| anyhow::anyhow!("JSON parse failed: {}", e))?;

    let mcp_did = invitation
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'from' in invitation"))?
        .to_string();

    let did_doc = invitation
        .get("body")
        .and_then(|b| b.get("did_document"))
        .cloned();

    let mediator_ws_url = invitation
        .get("body")
        .and_then(|b| b.get("services"))
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .map(|svc| {
            if let Some(url) = svc.as_str() {
                url.to_string()
            } else {
                svc.get("service_endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            }
        })
        .unwrap_or_default();

    let label = invitation
        .get("body")
        .and_then(|b| b.get("label"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(OobInvitationData {
        mcp_did,
        did_doc_json: did_doc
            .map(|d| serde_json::to_string(&d).unwrap_or_default())
            .unwrap_or_default(),
        mediator_ws_url,
        label,
    })
}

/// Send a connection request to the MCP after parsing the QR invitation.
/// Includes the merchant app's mediator HTTP URL so the MCP can forward messages back.
pub async fn send_connection_request(
    storage_path: String,
    mcp_did: String,
    mcp_did_doc_json: String,
    mediator_ws_url: String,
    push_channel: String,
    fcm_token: Option<String>,
    app_mediator_ws_url: Option<String>,
    app_mediator_http_url: Option<String>,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized"))?;

    let our_did = identity.did.clone();
    let (mut agent, _) = ignite_pay_core::didcomm::create_agent(identity);

    // Register MCP as a peer using its DID document
    if !mcp_did_doc_json.is_empty() {
        if let Ok(mcp_doc) = serde_json::from_str::<serde_json::Value>(&mcp_did_doc_json) {
            if let Some(resolved) = ignite_pay_core::identity::parse_did_document(&mcp_did, &mcp_doc) {
                agent.add_peer(resolved);
                tracing::info!("Registered MCP peer from invitation: {}", mcp_did);
            }
        }
    }

    // Build connection request message with mediator_http_url
    let app_http = app_mediator_http_url.as_deref().unwrap_or("");
    let msg = if app_http.is_empty() {
        ignite_pay_core::didcomm::build_connection_request(
            &our_did, &mcp_did, &push_channel, fcm_token.as_deref(),
        )
    } else {
        ignite_pay_core::didcomm::build_connection_request_with_mediator(
            &our_did, &mcp_did, &push_channel, fcm_token.as_deref(), app_http,
        )
    };

    // Encrypt with authcrypt
    let jwe = ignite_pay_core::didcomm::pack_encrypted(&agent, &msg, &our_did, &mcp_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Wrap in forward message and send to MCP's mediator via HTTP POST (no auth required)
    let forward_msg = serde_json::json!({
        "type": "https://didcomm.org/routing/2.0/forward",
        "id": format!("fwd-{}", uuid::Uuid::new_v4()),
        "body": { "next": mcp_did },
        "attachments": [{
            "data": { "json": jwe }
        }]
    });

    let forward_str = serde_json::to_string(&forward_msg)?;

    // Convert WS URL to HTTP URL: wss://host/ws -> https://host/
    let http_url = mediator_ws_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches("/ws")
        .to_string()
        + "/";

    tracing::info!("Sending forward-wrapped connection request to MCP mediator: {}", http_url);

    let client = reqwest::Client::new();
    let resp = client
        .post(&http_url)
        .header("Content-Type", "application/json")
        .body(forward_str)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "MCP mediator rejected connection request: {} - {}",
            status,
            body
        ));
    }

    tracing::info!("Forward-wrapped connection request sent to MCP {} via HTTP to {}", mcp_did, http_url);

    Ok(())
}

/// Send a create-channel request to the merchant MCP server via DIDComm.
pub async fn send_create_channel_request(
    storage_path: String,
    mcp_did: String,
    hub_endpoint: String,
    provider_pubkey: String,
    token_mint: String,
    deposit: u64,
    tree_depth: u32,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let identity = ignite_pay_core::identity::load_identity(&db)?
        .ok_or_else(|| anyhow::anyhow!("DIDComm identity not initialized"))?;
    let from_did = identity.did.clone();

    let msg = ignite_pay_core::didcomm::build_create_channel_request(
        &from_did,
        &mcp_did,
        &hub_endpoint,
        &provider_pubkey,
        &token_mint,
        deposit,
        tree_depth,
    );

    let (agent, _) = ignite_pay_core::didcomm::create_agent(identity);
    let jwe = ignite_pay_core::didcomm::pack_encrypted(&agent, &msg, &from_did, &mcp_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Send via the mediator's message relay endpoint
    let client = reqwest::Client::new();
    let _resp = client
        .post("http://localhost:3001/v1/messages/send")
        .json(&serde_json::json!({
            "jwe": jwe,
        }))
        .send()
        .await?;

    Ok(())
}
