// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use anyhow::Result;
use base64::Engine;
use once_cell::sync::Lazy;
use sha2::Digest;
use tokio::sync::Mutex;

use crate::api::identity::IdentityManager;
use crate::api::notification::{DecryptedMessage, DidcommMessage};
use crate::api::session::{
    MerchantPolicy, PaymentRecord, SessionKeyEntry, SessionKeyInfo, SessionOnChainInfo,
    TxHistoryEntry, UnsignedRegisterTx,
};
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

/// Register an MCP peer in the DIDComm agent using its DID document.
/// Must be called after pairing so the agent can decrypt authcrypt JWE
/// messages from this peer.
pub async fn register_mcp_peer(storage_path: String, mcp_did: String, mcp_did_doc_json: String) -> Result<()> {
    let mgr = IdentityManager::new(&storage_path)?;
    let agent = mgr.agent();

    if let Ok(mcp_doc) = serde_json::from_str::<serde_json::Value>(&mcp_did_doc_json) {
        if let Some(resolved) = ignite_pay_core::parse_did_document(&mcp_did, &mcp_doc) {
            let mut agent_guard = agent.lock().await;
            agent_guard.add_peer(resolved);
            tracing::info!("Registered MCP peer: {}", mcp_did);
            return Ok(());
        }
    }
    Err(anyhow::anyhow!("Failed to parse MCP DID document"))
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
    token_mint: Option<String>,
    payment_method: Option<String>,
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
        token_mint: None,
        payment_method,
    };

    if let Some(info) = &session_key_info {
        response.session_key_pubkey = Some(info.ephemeral_pubkey.clone());
        response.session_key_secret_key = Some(info.ephemeral_secret_key.clone());
        response.session_key_tx_signature = info.tx_signature.clone();
        response.session_expires_at = Some(info.expires_at);
        response.spending_limit = Some(info.spending_limit);
        response.scopes = Some(info.scopes.clone());
        response.token_mint = token_mint;
    }

    // Ensure MCP peer is registered in the WS client agent for encryption
    client.add_peer(&mcp_did).await;

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
/// Optionally accepts MCP peer DID + DID doc so authcrypt JWE from MCP can be verified.
pub fn decrypt_message(
    storage_path: String,
    jwe: String,
    mcp_did: Option<String>,
    mcp_did_doc_json: Option<String>,
) -> Result<DecryptedMessage> {
    let mgr = IdentityManager::new(&storage_path)?;
    let agent = mgr.agent();

    // Create a new Tokio runtime since this is called from a Dart thread
    // without an existing Tokio reactor.
    let rt = tokio::runtime::Runtime::new()?;
    let mut agent_guard = rt.block_on(agent.lock());

    // Register MCP peer if provided so authcrypt JWE can be decrypted.
    tracing::info!(
        "decrypt_message: mcp_did={:?}, has_doc={}",
        mcp_did,
        mcp_did_doc_json.is_some()
    );
    // Save sender DID for unpack_message before the if-let consumes it.
    let sender_did = mcp_did.clone();
    if let (Some(did), Some(doc_json)) = (mcp_did, mcp_did_doc_json) {
        match serde_json::from_str::<serde_json::Value>(&doc_json) {
            Ok(doc) => {
                match ignite_pay_core::parse_did_document(&did, &doc) {
                    Some(resolved) => {
                        tracing::info!(
                            "decrypt_message: add_peer did={}, kid={}",
                            did,
                            resolved.key_agreement_kid
                        );
                        agent_guard.add_peer(resolved);
                    }
                    None => {
                        tracing::error!(
                            "decrypt_message: parse_did_document returned None for did={}",
                            did
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!("decrypt_message: failed to parse DID doc JSON: {}", e);
            }
        }
    } else {
        tracing::warn!("decrypt_message: no MCP peer info provided");
    }

    let msg = ignite_pay_core::didcomm::unpack_message(&agent_guard, &jwe, sender_did.as_deref())
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
        token_mint: msg.body.get("token_mint")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| msg.body.get("sk_token_mint").and_then(|v| v.as_str()).map(String::from))
            .or_else(|| {
                msg.body.get("new_session_key")
                    .and_then(|sk| sk.get("token_mint"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            }),
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
        // F2: extract session key fields — try top-level first, then nested
        new_session_key_pubkey: msg.body.get("session_key_pubkey")
            .and_then(|v| v.as_str()).map(String::from)
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("session_key_pubkey")).and_then(|v| v.as_str()).map(String::from)),
        new_session_key_secret_key: msg.body.get("ephemeral_secret_key")
            .and_then(|v| v.as_str()).map(String::from)
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("ephemeral_secret_key")).and_then(|v| v.as_str()).map(String::from)),
        new_session_key_spending_limit: msg.body.get("spending_limit")
            .and_then(|v| v.as_u64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("spending_limit")).and_then(|v| v.as_u64())),
        new_session_key_duration_secs: msg.body.get("duration_secs")
            .and_then(|v| v.as_i64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("duration_secs")).and_then(|v| v.as_i64())),
        new_session_key_scopes: msg.body.get("scopes")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("scopes")).and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())),
        new_session_key_token_mint: msg.body.get("sk_token_mint")
            .and_then(|v| v.as_str()).map(String::from)
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("token_mint")).and_then(|v| v.as_str()).map(String::from)),
        new_session_key_suggested_sol_funding: msg.body.get("suggested_sol_funding")
            .and_then(|v| v.as_u64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("suggested_sol_funding")).and_then(|v| v.as_u64())),
        new_session_key_suggested_token_funding: msg.body.get("suggested_token_funding")
            .and_then(|v| v.as_u64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("suggested_token_funding")).and_then(|v| v.as_u64())),
        available_payment_methods: msg
            .body
            .get("available_payment_methods")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
        suggested_per_tx_limit: msg.body.get("suggested_per_tx_limit")
            .and_then(|v| v.as_u64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("suggested_per_tx_limit")).and_then(|v| v.as_u64())),
        suggested_daily_tx_count_limit: msg.body.get("suggested_daily_tx_count_limit")
            .and_then(|v| v.as_u64())
            .or_else(|| msg.body.get("new_session_key").and_then(|sk| sk.get("suggested_daily_tx_count_limit")).and_then(|v| v.as_u64()))
            .map(|v| v as u32),
        // F3/F7: session-fund-request fields
        session_fund_required_amount: msg.body.get("required_amount").and_then(|v| v.as_u64()),
        session_fund_current_balance: msg.body.get("current_balance").and_then(|v| v.as_u64()),
        session_fund_spending_limit_remaining: msg.body.get("spending_limit_remaining").and_then(|v| v.as_u64()),
        session_fund_token_mint: msg.body.get("token_mint").and_then(|v| v.as_str()).map(String::from),
        session_fund_reason: msg.body.get("reason").and_then(|v| v.as_str()).map(String::from),
        // F13: balance-notification fields
        balance_notification_balance: msg.body.get("balance").and_then(|v| v.as_u64()),
        balance_notification_threshold: msg.body.get("threshold").and_then(|v| v.as_u64()),
        balance_notification_spending_limit_remaining: msg.body.get("spending_limit_remaining").and_then(|v| v.as_u64()),
        // F14: session-renew-request fields
        old_session_key_pubkey: msg.body.get("old_session_key_pubkey").and_then(|v| v.as_str()).map(String::from),
        session_renew_expires_at: msg.body.get("expires_at").and_then(|v| v.as_i64()),
        // F16: Relayer payment method fields
        relayer_pubkey: msg.body.get("relayer_pubkey").and_then(|v| v.as_str()).map(String::from),
        relayer_url: msg.body.get("relayer_url").and_then(|v| v.as_str()).map(String::from),
    };

    Ok(decrypted)
}

/// Send a session fund response back to the MCP server.
pub async fn send_session_fund_response(
    _storage_path: String,
    mcp_did: String,
    session_key_pubkey: String,
    funded: bool,
    new_balance: u64,
    tx_signature: Option<String>,
) -> Result<()> {
    let global = GLOBAL_WS_CLIENT.lock().await;
    let client = global
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not connected to mediator"))?;

    let mgr = IdentityManager::new(&_storage_path)?;
    let our_did = mgr.did().to_string();
    let agent = mgr.agent();

    let msg = ignite_pay_core::didcomm::build_session_fund_response(
        &our_did,
        &mcp_did,
        &session_key_pubkey,
        funded,
        new_balance,
        tx_signature.as_deref(),
    );

    let jwe = {
        let agent_guard = agent.lock().await;
        ignite_pay_core::didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &mcp_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
    };

    client.send_raw(&jwe).await?;
    Ok(())
}

/// Send a session renew response back to the MCP server.
pub async fn send_session_renew_response(
    _storage_path: String,
    mcp_did: String,
    old_session_key_pubkey: String,
    new_session_key_pubkey: String,
    renewed: bool,
    tx_signature: Option<String>,
) -> Result<()> {
    let global = GLOBAL_WS_CLIENT.lock().await;
    let client = global
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not connected to mediator"))?;

    let mgr = IdentityManager::new(&_storage_path)?;
    let our_did = mgr.did().to_string();
    let agent = mgr.agent();

    let msg = ignite_pay_core::didcomm::build_session_renew_response(
        &our_did,
        &mcp_did,
        &old_session_key_pubkey,
        &new_session_key_pubkey,
        renewed,
        tx_signature.as_deref(),
    );

    let jwe = {
        let agent_guard = agent.lock().await;
        ignite_pay_core::didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &mcp_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
    };

    client.send_raw(&jwe).await?;
    Ok(())
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
    token_mint: Option<String>,
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

    let (target_program_str, scopes) = match &token_mint {
        Some(_) => {
            // SPL token session: target = Token program, scope = spl:transfer
            ("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
             vec!["spl:transfer".to_string()])
        }
        None => {
            // SOL session: target = System program, scope = sol:transfer
            ("11111111111111111111111111111111".to_string(),
             vec!["sol:transfer".to_string()])
        }
    };

    // Use session module to create the local session
    let session_info = crate::api::session::create_session_key(
        storage_path,
        owner_pubkey_str,
        target_program_str,
        scopes,
        spending_limit,
        duration_secs,
        0, // per_tx_limit: 0 = no limit
        0, // daily_tx_count_limit: 0 = no limit
    )?;

    Ok(session_info)
}

/// Authenticate with the mediator and get a JWT token.
/// Uses challenge-response: fetches a nonce, signs it with the DID's Ed25519 signing key, and exchanges for JWT.
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
    let mgr = IdentityManager::new(&storage_path)?;
    let signature_b64 = mgr.sign(nonce.as_bytes());

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
    // services can contain Map objects or plain URL strings (DIDComm OOB spec)
    let mediator_ws_url = invitation
        .get("body")
        .and_then(|b| b.get("services"))
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .map(|svc| {
            // If service is a plain string, use it directly
            if let Some(url) = svc.as_str() {
                url.to_string()
            } else {
                // Otherwise extract service_endpoint from the object
                svc.get("service_endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            }
        })
        .unwrap_or_default();

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

/// Register an externally-provided session key on-chain (MCP created the keypair).
pub async fn register_external_session_key(
    storage_path: String,
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    ephemeral_secret_key: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    token_mint: Option<String>,
) -> Result<SessionKeyInfo> {
    crate::api::session::register_external_session_key(
        storage_path,
        rpc_url,
        owner_secret_key,
        ephemeral_pubkey,
        ephemeral_secret_key,
        target_program,
        scopes,
        spending_limit,
        duration_secs,
        token_mint,
    )
    .await
}

/// Fund a session key by transferring SOL (and optionally SPL token) from the owner.
pub async fn fund_session_key(
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    sol_amount: u64,
    spl_token_mint: Option<String>,
    spl_amount: Option<u64>,
) -> Result<Vec<String>> {
    crate::api::session::fund_session_key(
        rpc_url,
        owner_secret_key,
        ephemeral_pubkey,
        sol_amount,
        spl_token_mint,
        spl_amount,
    )
    .await
}

/// Register an externally-provided session key and fund it in one operation.
pub async fn register_and_fund_session_key(
    storage_path: String,
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    ephemeral_secret_key: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    token_mint: Option<String>,
    sol_funding: u64,
    token_funding: Option<u64>,
) -> Result<SessionKeyInfo> {
    crate::api::session::register_and_fund_session_key(
        storage_path,
        rpc_url,
        owner_secret_key,
        ephemeral_pubkey,
        ephemeral_secret_key,
        target_program,
        scopes,
        spending_limit,
        duration_secs,
        token_mint,
        sol_funding,
        token_funding,
    )
    .await
}

/// Parsed OOB invitation data.
pub struct OobInvitationData {
    pub mcp_did: String,
    pub did_doc_json: String,
    pub mediator_ws_url: String,
    pub label: String,
}

/// Sign a nonce string with the phone's Ed25519 signing key.
/// Returns the base64-no-pad encoded signature.
pub fn sign_nonce(storage_path: String, nonce: String) -> Result<String> {
    let mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    Ok(mgr.sign(nonce.as_bytes()))
}

/// Verify an Ed25519 signature from a DID.
/// Returns true if the signature is valid for the given message and DID.
pub fn verify_did_signature(did: String, message: String, signature_b64: String) -> Result<bool> {
    Ok(ignite_pay_core::verify_did_signature(&did, &message, &signature_b64))
}

/// Drain all queued mediator messages received via the Rust WS connection.
/// Called periodically by the Dart layer to process incoming messages.
pub fn drain_mediator_messages() -> Result<Vec<String>> {
    Ok(crate::api::ws_client::drain_message_queue())
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
    let _our_did_doc = mgr.did_doc().clone();
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

// ── State Channel Bridge Wrappers ──────────────────────────────────────

/// Parse a payment QR code (ignite://pay?d=... format).
/// Returns merchant DID, amount, description, order ID, and hub endpoint.
pub fn parse_payment_qr(qr_data: String) -> Result<crate::api::channel::PaymentQrData> {
    crate::api::channel::parse_payment_qr(qr_data)
}

/// List all stored state channels.
pub fn list_channels(storage_path: String) -> Result<Vec<crate::api::channel_store::ChannelInfo>> {
    crate::api::channel::list_channels(storage_path)
}

/// Get channel state info.
pub fn get_channel_state(
    storage_path: String,
    channel_id: String,
) -> Result<crate::api::channel::ChannelStateInfo> {
    crate::api::channel::get_channel_state(storage_path, channel_id)
}

/// Open a state channel with a Hub.
pub async fn open_channel(
    storage_path: String,
    hub_endpoint: String,
    deposit: u64,
    tree_depth: u32,
) -> Result<crate::api::channel::OpenChannelResult> {
    crate::api::channel::open_channel(storage_path, hub_endpoint, deposit, tree_depth).await
}

/// Pay through a state channel.
pub async fn channel_pay(
    storage_path: String,
    channel_id: String,
    hub_endpoint: String,
    amount: u64,
    recipient_pubkey: String,
) -> Result<crate::api::channel::PaymentResult> {
    crate::api::channel::channel_pay(
        storage_path,
        channel_id,
        hub_endpoint,
        amount,
        recipient_pubkey,
    )
    .await
}

/// Close a state channel.
pub async fn close_channel(
    storage_path: String,
    channel_id: String,
) -> Result<String> {
    crate::api::channel::close_channel(storage_path, channel_id).await
}

/// Settle a state channel.
pub async fn settle_channel(
    storage_path: String,
    channel_id: String,
    hub_endpoint: String,
) -> Result<String> {
    crate::api::channel::settle_channel(storage_path, channel_id, hub_endpoint).await
}

// ── Hub Registry & Channel Creation ─────────────────────────────────────

/// Hub info from the registry.
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

/// Send a create-channel request to the MCP server via DIDComm.
pub async fn send_create_channel_request(
    storage_path: String,
    mcp_did: String,
    hub_endpoint: String,
    provider_pubkey: String,
    token_mint: String,
    deposit: u64,
    tree_depth: u32,
) -> Result<()> {
    let mgr = IdentityManager::new(&storage_path)?;
    let from_did = mgr.did().to_string();

    let msg = ignite_pay_core::didcomm::build_create_channel_request(
        &from_did,
        &mcp_did,
        &hub_endpoint,
        &provider_pubkey,
        &token_mint,
        deposit,
        tree_depth,
    );

    let agent = mgr.agent();
    let agent_guard = agent.lock().await;
    let jwe = ignite_pay_core::didcomm::pack_encrypted(&agent_guard, &msg, &from_did, &mcp_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
    drop(agent_guard);

    let ws = GLOBAL_WS_CLIENT.lock().await;
    let ws_client = ws
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
    ws_client.send_raw(&jwe).await?;

    Ok(())
}

// ── MB Voucher Bridge Wrappers ──────────────────────────────────────────

/// Send an MB deposit request to the MCP server.
/// The MCP deposits into the MagicBlock shared vault and returns mb-deposit-response.
pub async fn send_mb_deposit_request(
    storage_path: String,
    amount: u64,
    token: String,
) -> Result<()> {
    use ignite_pay_core::didcomm;
    use crate::api::identity::IdentityManager;

    let mgr = IdentityManager::new(&storage_path)?;
    let our_did = mgr.did().to_string();
    let agent = mgr.agent();

    // Resolve the MCP DID from paired connection
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("paired_mcp")?;
    let mcp_did = String::from_utf8(
        tree.get("mcp_did")?
            .ok_or_else(|| anyhow::anyhow!("No paired MCP found"))?
            .to_vec()
    )?;

    // Build the mb-deposit-request DIDComm message
    let msg = didcomm::build_mb_deposit_request(
        &our_did,
        &mcp_did,
        amount,
        &token,
    );

    // Encrypt to JWE
    let agent_guard = agent.lock().await;
    let jwe = didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &mcp_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
    drop(agent_guard);

    // Send via WebSocket to the mediator
    let ws = GLOBAL_WS_CLIENT.lock().await;
    let ws_client = ws
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
    ws_client.send_raw(&jwe).await?;

    tracing::info!(
        "MB deposit request sent: amount={} lamports",
        amount
    );

    Ok(())
}

/// Get or generate the buyer MB keypair, return base58 pubkey.
pub fn mb_get_buyer_pubkey(storage_path: String) -> Result<String> {
    crate::api::mb_voucher::get_mb_buyer_pubkey(storage_path)
}

/// Sign an MB voucher for payment.
pub fn mb_sign_voucher(
    storage_path: String,
    program_id: String,
    merchant_mb_pubkey: String,
    seq: u64,
    amount: u64,
) -> Result<crate::api::mb_voucher::MbVoucherResult> {
    crate::api::mb_voucher::sign_mb_voucher(
        storage_path,
        program_id,
        merchant_mb_pubkey,
        seq,
        amount,
    )
}

/// Send a QR payment request to the MCP server.
/// Called when user scans a merchant QR code, selects payment method, and confirms.
/// The MCP executes the payment and returns a qr-payment-response.
pub async fn send_qr_payment_request(
    storage_path: String,
    merchant_did: String,
    amount: u64,
    description: String,
    order_id: String,
    payment_method: String,
    token: String,
    merchant_mediator_url: String,
) -> Result<()> {
    use ignite_pay_core::didcomm;
    use crate::api::identity::IdentityManager;

    let mgr = IdentityManager::new(&storage_path)?;
    let our_did = mgr.did().to_string();
    let agent = mgr.agent();

    // Resolve the MCP DID from paired connection
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("paired_mcp")?;
    let mcp_did = String::from_utf8(
        tree.get("mcp_did")?
            .ok_or_else(|| anyhow::anyhow!("No paired MCP found"))?
            .to_vec()
    )?;

    // Build the qr-payment-request DIDComm message
    let msg = didcomm::build_qr_payment_request(
        &our_did,
        &mcp_did,
        &merchant_did,
        amount,
        &description,
        &order_id,
        &payment_method,
        &token,
        &merchant_mediator_url,
    );

    // Encrypt to JWE
    let agent_guard = agent.lock().await;
    let jwe = didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &mcp_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
    drop(agent_guard);

    // Send via WebSocket to the mediator
    let ws = GLOBAL_WS_CLIENT.lock().await;
    let ws_client = ws
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
    ws_client.send_raw(&jwe).await?;

    tracing::info!(
        "QR payment request sent: order={} merchant={} amount={} method={}",
        order_id, merchant_did, amount, payment_method
    );

    Ok(())
}

/// Build an unsigned SOL transfer transaction for direct wallet signing.
/// Bridge wrapper around `session::build_unsigned_transfer_tx`.
pub async fn build_unsigned_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_did: String,
    amount_lamports: u64,
) -> Result<String> {
    crate::api::session::build_unsigned_transfer_tx(
        rpc_url,
        wallet_pubkey_b58,
        merchant_did,
        amount_lamports,
    )
    .await
}

/// Send a signed MB voucher to the merchant via DIDComm.
pub async fn mb_send_voucher(
    storage_path: String,
    merchant_did: String,
    order_id: String,
    channel_id: String,
    seq: u64,
    amount: u64,
    buyer_pubkey: String,
    buyer_sig: String,
) -> Result<()> {
    let jwe = crate::api::mb_voucher::build_mb_voucher_jwe(
        storage_path,
        merchant_did,
        order_id,
        channel_id,
        seq,
        amount,
        buyer_pubkey,
        buyer_sig,
    ).await?;

    let ws = GLOBAL_WS_CLIENT.lock().await;
    let ws_client = ws
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
    ws_client.send_raw(&jwe).await?;

    Ok(())
}

// ── Sponsored (Relayer) Payment Bridge Wrappers ─────────────────────────

/// Fetch the relayer's fee-payer public key from GET /info.
pub async fn fetch_relayer_pubkey(relayer_url: String) -> Result<String> {
    crate::api::session::fetch_relayer_pubkey(relayer_url).await
}

/// Build an unsigned sponsored SOL transfer transaction for direct wallet signing.
/// Bridge wrapper around `session::build_unsigned_sponsored_transfer_tx`.
pub async fn build_unsigned_sponsored_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_did: String,
    amount_lamports: u64,
    relayer_pubkey_b58: String,
) -> Result<String> {
    crate::api::session::build_unsigned_sponsored_transfer_tx(
        rpc_url,
        wallet_pubkey_b58,
        merchant_did,
        amount_lamports,
        relayer_pubkey_b58,
    )
    .await
}

/// Build an unsigned SPL Token transfer transaction for direct wallet signing.
/// Bridge wrapper around `session::build_unsigned_spl_transfer_tx`.
pub async fn build_unsigned_spl_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_wallet_b58: String,
    amount: u64,
    token_mint_b58: String,
) -> Result<String> {
    crate::api::session::build_unsigned_spl_transfer_tx(
        rpc_url,
        wallet_pubkey_b58,
        merchant_wallet_b58,
        amount,
        token_mint_b58,
    )
    .await
}

/// Build an unsigned sponsored SPL Token transfer transaction for direct wallet signing.
/// Bridge wrapper around `session::build_unsigned_sponsored_spl_transfer_tx`.
pub async fn build_unsigned_sponsored_spl_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_wallet_b58: String,
    amount: u64,
    token_mint_b58: String,
    relayer_pubkey_b58: String,
) -> Result<String> {
    crate::api::session::build_unsigned_sponsored_spl_transfer_tx(
        rpc_url,
        wallet_pubkey_b58,
        merchant_wallet_b58,
        amount,
        token_mint_b58,
        relayer_pubkey_b58,
    )
    .await
}

// ── Balance, On-Chain Check, Payment Records ─────────────────────────

/// Get SOL balance (lamports) for a pubkey.
pub async fn get_sol_balance(rpc_url: String, pubkey_b58: String) -> Result<u64> {
    crate::api::session::get_sol_balance(rpc_url, pubkey_b58).await
}

/// Get SPL token balance for an owner + mint pair.
pub async fn get_token_balance(
    rpc_url: String,
    owner_pubkey_b58: String,
    token_mint_b58: String,
) -> Result<u64> {
    crate::api::session::get_token_balance(rpc_url, owner_pubkey_b58, token_mint_b58).await
}

/// Check if a session key PDA exists on-chain.
pub async fn get_session_account_info(
    rpc_url: String,
    owner_b58: String,
    ephemeral_b58: String,
) -> Result<SessionOnChainInfo> {
    crate::api::session::get_session_account_info(rpc_url, owner_b58, ephemeral_b58).await
}

/// Finalize an already-registered session key by persisting it to local storage.
pub fn finalize_existing_session_key(
    storage_path: String,
    owner_pubkey_b58: String,
    ephemeral_pubkey: String,
    on_chain_info: SessionOnChainInfo,
    scopes: Vec<String>,
) -> Result<SessionKeyInfo> {
    crate::api::session::finalize_existing_session_key(
        storage_path,
        owner_pubkey_b58,
        ephemeral_pubkey,
        on_chain_info,
        scopes,
    )
}

/// Derive the owner's Solana pubkey from the DID stored in sled.
pub fn get_owner_pubkey(storage_path: String) -> Result<String> {
    crate::api::session::get_owner_pubkey(storage_path)
}

/// Save a payment authorization record to sled.
pub fn save_payment_record(storage_path: String, record: PaymentRecord) -> Result<()> {
    crate::api::session::save_payment_record(storage_path, record)
}

/// List all payment records from sled, newest-first.
pub fn list_payment_records(storage_path: String) -> Result<Vec<PaymentRecord>> {
    crate::api::session::list_payment_records(storage_path)
}

/// Get recent transaction signatures for a pubkey.
pub async fn get_transaction_history(
    rpc_url: String,
    pubkey_b58: String,
    limit: u32,
) -> Result<Vec<TxHistoryEntry>> {
    crate::api::session::get_transaction_history(rpc_url, pubkey_b58, limit).await
}

// ── CCTP Forwarding ─────────────────────────────────────────────────────────

/// Query CCTP forwarding fees from Circle Iris API.
pub async fn cctp_get_fees(
    iris_api_url: String,
    src_domain: u32,
    dst_domain: u32,
) -> Result<crate::api::cctp_transfer::CctpFeeQuote> {
    crate::api::cctp_transfer::cctp_get_fees(iris_api_url, src_domain, dst_domain).await
}

/// Build ERC-20 approve calldata (USDC → TokenMessengerV2).
pub fn cctp_build_approve_calldata(spender: String, amount: u64) -> Result<String> {
    crate::api::cctp_transfer::cctp_build_approve_calldata(spender, amount)
}

/// Build depositForBurnWithHook calldata for TokenMessengerV2.
pub fn cctp_build_deposit_for_burn_calldata(
    amount: u64,
    dst_domain: u32,
    mint_recipient: String,
    burn_token: String,
    dst_caller: String,
    max_fee: u32,
    min_finality_threshold: u32,
) -> Result<String> {
    crate::api::cctp_transfer::cctp_build_deposit_for_burn_calldata(
        amount,
        dst_domain,
        mint_recipient,
        burn_token,
        dst_caller,
        max_fee,
        min_finality_threshold,
    )
}

/// Derive the Solana USDC ATA for a wallet address (returns hex bytes32).
pub fn cctp_derive_solana_usdc_ata(wallet_b58: String) -> Result<String> {
    crate::api::cctp_transfer::cctp_derive_solana_usdc_ata(wallet_b58)
}

/// Poll Circle Iris API for CCTP transfer status.
pub async fn cctp_poll_status(
    iris_api_url: String,
    src_domain: u32,
    burn_tx_hash: String,
) -> Result<crate::api::cctp_transfer::CctpTransferStatus> {
    crate::api::cctp_transfer::cctp_poll_status(iris_api_url, src_domain, burn_tx_hash).await
}

// ── Merchant Profile Resolution ──────────────────────────────────────────

/// Merchant profile resolved from the DID Registry.
pub struct MerchantProfile {
    pub did: String,
    pub verified: bool,
    pub name: Option<String>,
    pub category: Option<String>,
}

/// Fetch a merchant profile from the DID Registry.
/// On network error or 404, returns an unverified profile with name=None.
pub async fn fetch_merchant_profile(
    registry_url: String,
    merchant_did: String,
) -> Result<MerchantProfile> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/merchants/profile/{}", registry_url, merchant_did);

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to fetch merchant profile: {}", e);
            return Ok(MerchantProfile {
                did: merchant_did,
                verified: false,
                name: None,
                category: None,
            });
        }
    };

    if !resp.status().is_success() {
        tracing::warn!("Merchant profile not found: {}", resp.status());
        return Ok(MerchantProfile {
            did: merchant_did,
            verified: false,
            name: None,
            category: None,
        });
    }

    let body: serde_json::Value = resp.json().await?;
    Ok(MerchantProfile {
        did: body["did"].as_str().unwrap_or(&merchant_did).to_string(),
        verified: body["verified"].as_bool().unwrap_or(false),
        name: body["name"].as_str().map(String::from),
        category: body["category"].as_str().map(String::from),
    })
}

/// Cache a merchant profile in sled under `merchant_profile:{did}`.
pub fn save_cached_merchant_profile(
    storage_path: String,
    profile: MerchantProfile,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let key = format!("merchant_profile:{}", profile.did);
    let value = serde_json::json!({
        "did": profile.did,
        "verified": profile.verified,
        "name": profile.name,
        "category": profile.category,
    });
    db.insert(key.as_bytes(), serde_json::to_vec(&value)?)?;
    Ok(())
}

/// Load a cached merchant profile from sled. Returns `None` if not cached.
pub fn load_cached_merchant_profile(
    storage_path: String,
    merchant_did: String,
) -> Result<Option<MerchantProfile>> {
    let db = sled::open(&storage_path)?;
    let key = format!("merchant_profile:{}", merchant_did);
    match db.get(key.as_bytes())? {
        Some(value) => {
            let v: serde_json::Value = serde_json::from_slice(&value)?;
            Ok(Some(MerchantProfile {
                did: v["did"].as_str().unwrap_or(&merchant_did).to_string(),
                verified: v["verified"].as_bool().unwrap_or(false),
                name: v["name"].as_str().map(String::from),
                category: v["category"].as_str().map(String::from),
            }))
        }
        None => Ok(None),
    }
}
