use anyhow::Result;
use base64::Engine;
use once_cell::sync::Lazy;
use sha2::Digest;
use tokio::sync::Mutex;

use crate::api::identity::IdentityManager;
use crate::api::notification::{DecryptedMessage, DidcommMessage};
use crate::api::session::{MerchantPolicy, SessionKeyEntry, SessionKeyInfo, UnsignedRegisterTx};
use crate::api::ws_client::WsClient;

// ── Global state ────────────────────────────────────────────────────────

static GLOBAL_WS_CLIENT: Lazy<Mutex<Option<WsClient>>> = Lazy::new(|| Mutex::new(None));
static GLOBAL_IDENTITY: Lazy<Mutex<Option<IdentityManager>>> = Lazy::new(|| Mutex::new(None));

// ── Return types ────────────────────────────────────────────────────────

/// Return type for the DID identity info.
pub struct DidInfo {
    pub did: String,
    pub did_doc_json: String,
}

/// Auth grant returned from payment signing.
pub struct AuthGrant {
    pub merchant_did: String,
    pub amount: u64,
    pub signature: String,
}

// ── Bridge functions ────────────────────────────────────────────────────

/// Initialize identity - generates or loads DID from storage.
/// Returns the DID string and DID document JSON.
pub fn initialize_identity(storage_path: String) -> Result<DidInfo> {
    let mgr = IdentityManager::new(&storage_path)?;

    // Store in global state for reuse.
    // Use or_create runtime to avoid depending on an existing Tokio reactor.
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut global = GLOBAL_IDENTITY.lock().await;
        *global = Some(IdentityManager::new(&storage_path)?);
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(DidInfo {
        did: mgr.did().to_string(),
        did_doc_json: serde_json::to_string(mgr.did_doc())?,
    })
}

/// Get the current DID from the initialized identity.
pub fn get_did(storage_path: String) -> Result<String> {
    let mgr = IdentityManager::new(&storage_path)?;
    Ok(mgr.did().to_string())
}

/// Connect to mediator WebSocket with auto-reconnect.
pub async fn connect_mediator(storage_path: String, ws_url: String) -> Result<()> {
    let mgr = IdentityManager::new(&storage_path)?;

    // Store in global state
    {
        let mut global = GLOBAL_WS_CLIENT.lock().await;
        *global = Some(WsClient::new(&mgr));
    }

    // Connect (lock is released before await via the block scope)
    {
        let global = GLOBAL_WS_CLIENT.lock().await;
        if let Some(ref client) = *global {
            client.connect(&ws_url).await?;
        }
    }

    // Store identity in global state
    {
        let mut global = GLOBAL_IDENTITY.lock().await;
        *global = Some(mgr);
    }

    Ok(())
}

/// Disconnect from the mediator.
pub async fn disconnect_mediator() -> Result<()> {
    let mut global = GLOBAL_WS_CLIENT.lock().await;
    *global = None;
    Ok(())
}

/// Send a payment authorization response back to the MCP server.
pub async fn send_auth_response(
    _storage_path: String,
    payment_id: String,
    authorized: bool,
    list_action: String,
    mcp_did: String,
    session_key_info: Option<SessionKeyInfo>,
    list_label: Option<String>,
    list_max_amount: Option<u64>,
    daily_tx_count_limit: Option<u32>,
    per_tx_limit: Option<u64>,
) -> Result<()> {
    let global = GLOBAL_WS_CLIENT.lock().await;
    let client = global
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not connected to mediator"))?;

    let mut response = crate::api::auth::AuthResponse {
        payment_id,
        authorized,
        list_action,
        session_key_pubkey: None,
        session_key_secret_key: None,
        session_key_tx_signature: None,
        session_expires_at: None,
        spending_limit: None,
        scopes: None,
        list_label,
        list_max_amount,
        daily_tx_count_limit,
        per_tx_limit,
    };

    if let Some(info) = &session_key_info {
        response.session_key_pubkey = Some(info.ephemeral_pubkey.clone());
        response.session_key_secret_key = Some(info.ephemeral_secret_key.clone());
        response.session_key_tx_signature = info.tx_signature.clone();
        response.session_expires_at = Some(info.expires_at);
        response.spending_limit = Some(info.spending_limit);
        response.scopes = Some(info.scopes.clone());
    }

    client.send_auth_response(&response, &mcp_did).await?;
    Ok(())
}

/// Poll for messages via HTTPS (for FCM wake-up path).
/// Returns a list of DIDComm message envelopes.
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

/// Decrypt a JWE message using the local identity.
pub fn decrypt_message(storage_path: String, jwe: String) -> Result<DecryptedMessage> {
    let mgr = IdentityManager::new(&storage_path)?;
    let agent = mgr.agent();

    // We need to use blocking code here since the bridge function is sync.
    // Use tokio's block_in_place to avoid blocking the runtime.
    let agent_guard =
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(agent.lock()));

    let msg = ignite_pay_core::didcomm::unpack_message(&agent_guard, &jwe, None)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
    drop(agent_guard);

    let msg_type = msg.typ.clone();
    let raw_body = serde_json::to_string(&msg.body)?;

    let decrypted = DecryptedMessage {
        msg_type,
        payment_id: msg
            .body
            .get("payment_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        merchant_did: msg
            .body
            .get("merchant_did")
            .and_then(|v| v.as_str())
            .map(String::from),
        amount: msg.body.get("amount").and_then(|v| v.as_u64()),
        description: msg
            .body
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        list_cid: msg
            .body
            .get("new_cid")
            .and_then(|v| v.as_str())
            .map(String::from),
        action: msg
            .body
            .get("action")
            .and_then(|v| v.as_str())
            .map(String::from),
        target_did: msg
            .body
            .get("entry_did")
            .and_then(|v| v.as_str())
            .map(String::from),
        raw_body,
        list_type: msg
            .body
            .get("list_type")
            .and_then(|v| v.as_str())
            .map(String::from),
        label: msg
            .body
            .get("list_label")
            .and_then(|v| v.as_str())
            .map(String::from),
    };

    Ok(decrypted)
}

/// Mock payment signing (placeholder for real signing).
pub async fn sign_payment(merchant_did: String, amount: u64) -> Result<AuthGrant> {
    let mock_signature = format!("sig_of_{}_for_{}", merchant_did, amount);
    Ok(AuthGrant {
        merchant_did,
        amount,
        signature: mock_signature,
    })
}

/// Create a local session key for payment authorization (V2.0).
/// Returns session key info that should be sent to the MCP server via auth response.
pub fn create_session_key_for_payment(
    storage_path: String,
    spending_limit: u64,
    duration_secs: i64,
) -> Result<SessionKeyInfo> {
    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
    let owner_keypair = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let owner_pubkey = owner_keypair.verifying_key();
    let owner_pubkey_str = bs58::encode(owner_pubkey.to_bytes()).into_string();

    // System program ID as base58 string
    let target_program_str = "11111111111111111111111111111111".to_string(); // System Program

    // Use session module to create the local session
    let session_info = crate::api::session::create_session_key(
        storage_path,
        owner_pubkey_str,
        target_program_str,
        vec!["sol:transfer".to_string()],
        spending_limit,
        duration_secs,
    )?;

    Ok(session_info)
}

/// Authenticate with the mediator and get a JWT token.
/// Uses challenge-response: fetches a nonce, signs it with the DID key, and exchanges for JWT.
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

    // Step 2: Sign the nonce with the DID's Ed25519 key
    // Derive the signing key from the DID (deterministic, same derivation as create_session_key_for_payment)
    let seed = sha2::Sha256::digest(did.as_bytes());
    let seed_bytes: &[u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed_bytes);
    use ed25519_dalek::Signer;
    let signature = signing_key.sign(nonce.as_bytes());
    let signature_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(signature.to_bytes());

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

/// Parse an OOB invitation URL (from QR code scan).
/// Extracts the MCP DID, DID document, and mediator WS URL from the invitation.
pub fn parse_oob_invitation(invitation_url: String) -> Result<OobInvitationData> {
    // Expected format: didcomm://?_oob=<base64url-encoded JSON>
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

    // Extract MCP DID (from "from" field)
    let mcp_did = invitation
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'from' in invitation"))?
        .to_string();

    // Extract DID document from body
    let did_doc = invitation
        .get("body")
        .and_then(|b| b.get("did_document"))
        .cloned();

    // Extract mediator WS URL from services
    let mediator_ws_url = invitation
        .get("body")
        .and_then(|b| b.get("services"))
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|svc| svc.get("service_endpoint"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Extract label
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

// ── Session Key Management Bridge Wrappers ─────────────────────────────

/// List all session keys stored locally.
pub fn list_session_keys(storage_path: String) -> Result<Vec<SessionKeyEntry>> {
    crate::api::session::list_session_keys(storage_path)
}

/// Find the first active session key.
pub fn find_active_session_key(storage_path: String) -> Result<Option<SessionKeyEntry>> {
    crate::api::session::find_active_session_key(storage_path)
}

/// Build an unsigned register-session-key transaction for external wallet signing.
pub async fn build_unsigned_register_tx(
    storage_path: String,
    rpc_url: String,
    spending_limit: u64,
    duration_secs: i64,
) -> Result<UnsignedRegisterTx> {
    crate::api::session::build_unsigned_register_tx(storage_path, rpc_url, spending_limit, duration_secs).await
}

/// Complete registration after receiving the owner signature from an external wallet.
pub async fn complete_register_with_signature(
    storage_path: String,
    ephemeral_pubkey: String,
    owner_signature_b58: String,
    rpc_url: String,
) -> Result<SessionKeyInfo> {
    crate::api::session::complete_register_with_signature(
        storage_path,
        ephemeral_pubkey,
        owner_signature_b58,
        rpc_url,
    )
    .await
}

/// Revoke a session key on-chain.
pub async fn revoke_session_key_onchain(
    storage_path: String,
    session_pubkey: String,
    rpc_url: String,
) -> Result<String> {
    crate::api::session::revoke_session_key_onchain(storage_path, session_pubkey, rpc_url).await
}

/// Delete a session key from local storage only.
pub fn delete_session_key_local(storage_path: String, session_pubkey: String) -> Result<()> {
    crate::api::session::delete_session_key_local(storage_path, session_pubkey)
}

/// Parsed OOB invitation data.
pub struct OobInvitationData {
    pub mcp_did: String,
    pub did_doc_json: String,
    pub mediator_ws_url: String,
    pub label: String,
}

/// Send a connection request to the MCP via the mediator.
/// This is called after parsing the QR code invitation.
pub async fn send_connection_request(
    storage_path: String,
    mcp_did: String,
    mcp_did_doc_json: String,
    mediator_ws_url: String,
    push_channel: String,
    fcm_token: Option<String>,
) -> Result<()> {
    // Get our identity
    let mgr = IdentityManager::new(&storage_path)?;
    let our_did = mgr.did().to_string();
    let our_did_doc = mgr.did_doc().clone();
    let agent = mgr.agent();

    // Register MCP as a peer using its DID document
    if !mcp_did_doc_json.is_empty() {
        if let Ok(mcp_doc) = serde_json::from_str::<serde_json::Value>(&mcp_did_doc_json) {
            if let Some(resolved) = ignite_pay_core::parse_did_document(&mcp_did, &mcp_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered MCP peer from invitation: {}", mcp_did);
            }
        }
    }

    // Build connection request message
    let msg = ignite_pay_core::didcomm::build_connection_request(
        &our_did,
        &mcp_did,
        &push_channel,
        fcm_token.as_deref(),
    );

    // Encrypt with authcrypt
    let jwe = {
        let agent_guard = agent.lock().await;
        ignite_pay_core::didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &mcp_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
    };

    // Send via the WS client (if connected) or via HTTP
    {
        let global = GLOBAL_WS_CLIENT.lock().await;
        if let Some(ref client) = *global {
            // Send through the existing WS connection
            client.send_raw(&jwe).await?;
            tracing::info!("Connection request sent to MCP {} via WS", mcp_did);
        } else {
            drop(global);
            // Not connected via WS — connect and send via HTTP to mediator
            let client = reqwest::Client::new();
            let url = format!(
                "{}/v1/agents/{}/command",
                mediator_ws_url
                    .replace("ws://", "http://")
                    .replace("wss://", "https://")
                    .trim_end_matches("/ws"),
                mcp_did
            );
            let response = client
                .post(&url)
                .json(&serde_json::json!({
                    "jwe_envelope": jwe
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "Connection request failed: {} - {}",
                    status,
                    body
                ));
            }
            tracing::info!("Connection request sent to MCP {} via HTTP", mcp_did);
        }
    }

    Ok(())
}

// ── Merchant Policy Bridge Wrappers ────────────────────────────────────

/// Save a merchant policy to sled.
pub fn save_merchant_policy(
    storage_path: String,
    merchant_did: String,
    daily_spending_limit: u64,
    daily_tx_count_limit: u32,
    per_tx_limit: u64,
    duration_secs: i64,
) -> Result<()> {
    crate::api::session::save_merchant_policy(
        storage_path,
        merchant_did,
        daily_spending_limit,
        daily_tx_count_limit,
        per_tx_limit,
        duration_secs,
    )
}

/// Load a merchant policy from sled.
pub fn load_merchant_policy(
    storage_path: String,
    merchant_did: String,
) -> Result<Option<MerchantPolicy>> {
    crate::api::session::load_merchant_policy(storage_path, merchant_did)
}
