use anyhow::Result;
use base64::Engine;
use once_cell::sync::Lazy;
use sha2::Digest;
use tokio::sync::Mutex;

use crate::api::identity::IdentityManager;
use crate::api::notification::{DecryptedMessage, DidcommMessage};
use crate::api::session::SessionKeyInfo;
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

    // Store in global state for reuse
    {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut global = GLOBAL_IDENTITY.lock().await;
            *global = Some(IdentityManager::new(&storage_path)?);
            Ok::<(), anyhow::Error>(())
        })?;
    }

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
