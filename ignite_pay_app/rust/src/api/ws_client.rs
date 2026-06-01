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

use crate::api::auth::{AuthRequest, AuthResponse};
use crate::api::identity::IdentityManager;

use affinidi_messaging_didcomm::DIDCommAgent;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use ignite_pay_core::didcomm::{self, is_jwe};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::connect_async;

/// Global queue for forwarding raw WS messages to the Dart layer.
/// The Dart layer polls this via `drain_mediator_messages()`.
static INCOMING_MESSAGE_QUEUE: Lazy<StdMutex<Vec<String>>> =
    Lazy::new(|| StdMutex::new(Vec::new()));

/// Queue a raw message for Dart consumption.
fn queue_incoming_message(msg: &str) {
    INCOMING_MESSAGE_QUEUE.lock().unwrap().push(msg.to_string());
}

/// Drain all queued messages (called from Dart via simple.rs).
pub fn drain_message_queue() -> Vec<String> {
    let mut queue = INCOMING_MESSAGE_QUEUE.lock().unwrap();
    std::mem::take(&mut *queue)
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Callback type for incoming auth requests.
pub type AuthCallback = Box<dyn Fn(AuthRequest) + Send + Sync>;

/// WebSocket client that connects to the DIDComm mediator.
pub struct WsClient {
    agent: Arc<Mutex<DIDCommAgent>>,
    our_did: String,
    did_doc: Value,
    signing_private: [u8; 32],
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    pending_auth_callback: Arc<Mutex<Option<AuthCallback>>>,
}

impl WsClient {
    pub fn new(identity_mgr: &IdentityManager) -> Self {
        Self {
            agent: identity_mgr.agent(),
            our_did: identity_mgr.did().to_string(),
            did_doc: identity_mgr.did_doc().clone(),
            signing_private: identity_mgr.signing_key().clone(),
            outgoing: Arc::new(Mutex::new(None)),
            pending_auth_callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the callback for incoming auth requests.
    pub async fn set_auth_callback(&self, callback: AuthCallback) {
        let mut cb = self.pending_auth_callback.lock().await;
        *cb = Some(callback);
    }

    /// Connect to the mediator: performs initial handshake synchronously
    /// (so errors propagate to the caller), then spawns a background task
    /// for the bidirectional message loop with auto-reconnect.
    pub async fn connect(&self, ws_url: &str) -> Result<()> {
        // Phase 0+A: connect + authenticate + mediation handshake (inline)
        let ws = connect_phase(ws_url, &self.agent, &self.our_did, &self.did_doc, &self.signing_private).await?;

        // Set up outgoing channel
        let (out_tx, out_rx) = mpsc::unbounded_channel::<String>();
        {
            let mut guard = self.outgoing.lock().await;
            *guard = Some(out_tx);
        }

        // Clone state for the background loop
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let signing_private = self.signing_private;
        let outgoing = self.outgoing.clone();
        let callback = self.pending_auth_callback.clone();
        let ws_url = ws_url.to_string();

        // Phase B: bidirectional loop + auto-reconnect (background)
        tokio::spawn(async move {
            let mut ws = ws;
            let mut out_rx = out_rx;
            loop {
                loop_phase(&mut ws, &agent, &callback, &mut out_rx).await;
                // Disconnected — clear outgoing channel
                {
                    let mut guard = outgoing.lock().await;
                    *guard = None;
                }
                drop(ws);
                loop {
                    tracing::warn!("Mediator disconnected, reconnecting in 3s...");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    match connect_phase(&ws_url, &agent, &our_did, &did_doc, &signing_private).await {
                        Ok(new_ws) => {
                            // Re-create outgoing channel
                            let (out_tx2, out_rx2) = mpsc::unbounded_channel::<String>();
                            {
                                let mut guard = outgoing.lock().await;
                                *guard = Some(out_tx2);
                            }
                            ws = new_ws;
                            out_rx = out_rx2;
                            break; // re-enter outer loop
                        }
                        Err(e) => {
                            tracing::error!("Reconnect failed: {}, retrying in 5s...", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Send an authorization response back through the mediator.
    pub async fn send_auth_response(&self, response: &AuthResponse, mcp_did: &str) -> Result<()> {
        // Build session key data if present
        let session_key_data = if response.session_key_pubkey.is_some()
            && response.session_key_secret_key.is_some()
        {
            Some(ignite_pay_core::didcomm::SessionKeyResponseData {
                session_key_pubkey: response.session_key_pubkey.clone().unwrap_or_default(),
                session_key_secret_key: response.session_key_secret_key.clone().unwrap_or_default(),
                session_key_tx_signature: response
                    .session_key_tx_signature
                    .clone()
                    .unwrap_or_default(),
                session_expires_at: response.session_expires_at.unwrap_or(0),
                spending_limit: response.spending_limit.unwrap_or(0),
                scopes: response.scopes.clone().unwrap_or_default(),
                daily_tx_count_limit: response.daily_tx_count_limit.unwrap_or(0),
                per_tx_limit: response.per_tx_limit.unwrap_or(0),
                token_mint: response.token_mint.clone(),
            })
        } else {
            None
        };

        let msg = didcomm::build_authorization_response_v1_1(
            &self.our_did,
            mcp_did,
            &response.payment_id,
            response.authorized,
            &response.list_action,
            session_key_data.as_ref(),
            response.list_label.as_deref(),
            response.list_max_amount,
        );

        let jwe = {
            let agent = self.agent.lock().await;
            didcomm::pack_encrypted(&agent, &msg, &self.our_did, mcp_did)
                .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
        };

        let outgoing_guard = self.outgoing.lock().await;
        if let Some(sender) = outgoing_guard.as_ref() {
            sender
                .send(jwe)
                .map_err(|_| anyhow::anyhow!("WS channel closed"))?;
        }

        Ok(())
    }

    /// Notify a paired peer that our mediator endpoint changed.
    pub async fn send_mediator_update(
        &self,
        peer_did: &str,
        mediator_http_url: &str,
        mediator_ws_url: &str,
    ) -> Result<()> {
        let msg = didcomm::build_mediator_update(
            &self.our_did,
            peer_did,
            mediator_http_url,
            Some(mediator_ws_url),
        );

        let jwe = {
            let agent = self.agent.lock().await;
            didcomm::pack_encrypted(&agent, &msg, &self.our_did, peer_did)
                .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
        };

        let outgoing_guard = self.outgoing.lock().await;
        if let Some(sender) = outgoing_guard.as_ref() {
            sender
                .send(jwe)
                .map_err(|_| anyhow::anyhow!("WS channel closed"))?;
        }

        Ok(())
    }

    /// Register a peer DID in the agent for encryption.
    pub async fn add_peer(&self, peer_did: &str) {
        let peer_identity =
            affinidi_messaging_didcomm::identity::PrivateIdentity::generate(peer_did);
        let resolved = ignite_pay_core::identity_to_resolved(&peer_identity);
        let mut agent = self.agent.lock().await;
        agent.add_peer(resolved);
    }

    /// Send a raw JWE string through the outgoing WS channel.
    pub async fn send_raw(&self, jwe: &str) -> Result<()> {
        let outgoing_guard = self.outgoing.lock().await;
        if let Some(sender) = outgoing_guard.as_ref() {
            sender
                .send(jwe.to_string())
                .map_err(|_| anyhow::anyhow!("WS channel closed"))?;
        } else {
            return Err(anyhow::anyhow!("Not connected to mediator"));
        }
        Ok(())
    }
}

/// Phase 0+A: Connect, authenticate, and complete mediation handshake.
/// Returns the authenticated WebSocket stream on success.
/// Errors propagate to the caller so the Dart layer knows if connection failed.
async fn connect_phase(
    ws_url: &str,
    _agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    signing_private: &[u8; 32],
) -> Result<WsStream> {
    let (mut ws, _) = connect_async(ws_url).await?;
    tracing::info!("WS connected to mediator: {}", ws_url);

    // --- Phase 0: Challenge-Response Authentication ---

    // Wait for ws-challenge message
    let challenge_text = read_msg_with_timeout(&mut ws, Duration::from_secs(10))
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for WS challenge"))?;
    let challenge: Value = serde_json::from_str(&challenge_text)?;
    if challenge
        .get("type")
        .and_then(|v| v.as_str())
        != Some("https://didcomm.org/ignite-pay/1.0/ws-challenge")
    {
        return Err(anyhow::anyhow!(
            "Expected ws-challenge, got: {}",
            challenge_text.chars().take(100).collect::<String>()
        ));
    }

    let nonce = challenge["body"]["nonce"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing nonce in challenge"))?;

    // Sign the nonce with our Ed25519 signing key
    let signature_b64 = ignite_pay_core::sign_message(signing_private, nonce.as_bytes());

    // Send signed challenge-response (plaintext, no JWE)
    let response = serde_json::json!({
        "type": "https://didcomm.org/ignite-pay/1.0/ws-challenge-response",
        "id": uuid::Uuid::new_v4().to_string(),
        "from": our_did,
        "body": {
            "nonce": nonce,
            "signature": signature_b64,
            "did_document": did_doc,
        }
    });
    send_msg(&mut ws, serde_json::to_string(&response)?).await?;
    tracing::info!("Sent WS challenge-response (signed)");

    // Wait for auth-ok
    let auth_result = read_msg_with_timeout(&mut ws, Duration::from_secs(5))
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for auth result"))?;
    let auth_v: Value = serde_json::from_str(&auth_result)?;
    match auth_v.get("type").and_then(|v| v.as_str()) {
        Some(t) if t.contains("ws-auth-ok") => {
            tracing::info!("WS authentication successful");
        }
        Some(t) if t.contains("ws-auth-failed") => {
            let reason = auth_v["body"]["reason"].as_str().unwrap_or("unknown");
            return Err(anyhow::anyhow!("WS auth failed: {}", reason));
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unexpected auth response: {}",
                auth_result.chars().take(100).collect::<String>()
            ));
        }
    }

    // --- Phase A: Mediation handshake ---
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;

    let grant = read_msg(&mut ws).await?;
    let grant_v: Value = serde_json::from_str(&grant)?;
    if !grant_v
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| t.contains("mediate-grant"))
        .unwrap_or(false)
    {
        tracing::warn!("Expected mediate-grant, got: {}", grant);
    }

    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;

    let kl_resp = read_msg(&mut ws).await?;
    let kl_v: Value = serde_json::from_str(&kl_resp)?;
    if !kl_v
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| t.contains("keylist-update"))
        .unwrap_or(false)
    {
        tracing::warn!("Expected keylist-update-response, got: {}", kl_resp);
    }

    let intro = didcomm::build_peer_introduction(our_did, did_doc);
    send_msg(&mut ws, serde_json::to_string(&intro)?).await?;

    tracing::info!("Mediator handshake complete");
    Ok(ws)
}

/// Phase B: Bidirectional message loop.
/// Reads incoming messages and forwards them; sends outgoing messages from the channel.
/// Returns when the connection is closed or an error occurs.
async fn loop_phase(
    ws: &mut WsStream,
    agent: &Arc<Mutex<DIDCommAgent>>,
    callback: &Arc<Mutex<Option<AuthCallback>>>,
    out_rx: &mut mpsc::UnboundedReceiver<String>,
) {
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_message(&text, agent, callback).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::error!("WS read error: {}", e);
                        return;
                    }
                    None => return,
                }
            }
            jwe = out_rx.recv() => {
                match jwe {
                    Some(msg) => {
                        if let Err(e) = send_msg(ws, msg).await {
                            tracing::error!("WS send error: {}", e);
                            return;
                        }
                    }
                    None => return,
                }
            }
        }
    }
}

async fn handle_message(
    text: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    callback: &Arc<Mutex<Option<AuthCallback>>>,
) {
    // Forward ALL raw messages to Dart layer for processing
    queue_incoming_message(text);

    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                if msg.typ.contains("payment-auth-request") {
                    if let (Some(pid), Some(merchant), Some(amount)) = (
                        msg.body.get("payment_id").and_then(|v| v.as_str()),
                        msg.body.get("merchant_did").and_then(|v| v.as_str()),
                        msg.body.get("amount").and_then(|v| v.as_u64()),
                    ) {
                        let auth_req = AuthRequest {
                            payment_id: pid.to_string(),
                            merchant_did: merchant.to_string(),
                            amount,
                            description: msg
                                .body
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        };
                        let cb_guard = callback.lock().await;
                        if let Some(cb) = cb_guard.as_ref() {
                            cb(auth_req);
                        }
                    }
                } else if msg.typ.contains("list-sync-notification") {
                    // V1.1: Handle list sync notification
                    let list_cid = msg
                        .body
                        .get("new_cid")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let action = msg
                        .body
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let target_did = msg
                        .body
                        .get("entry_did")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    tracing::info!(
                        "List sync notification: cid={}, action={}, target={}",
                        list_cid,
                        action,
                        target_did
                    );
                    // TODO: Store/update local list cache from IPFS CID when IpfsClient is available
                }
                return;
            }
            Err(e) => {
                tracing::warn!("JWE unpack failed: {}", e);
                drop(agent_guard);
            }
        }
    }

    // Plaintext fallback
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(body) = v.get("body") {
            if body.get("payment_id").is_some() {
                let auth_req = AuthRequest {
                    payment_id: body
                        .get("payment_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    merchant_did: body
                        .get("merchant_did")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    amount: body.get("amount").and_then(|v| v.as_u64()).unwrap_or(0),
                    description: body
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                let cb_guard = callback.lock().await;
                if let Some(cb) = cb_guard.as_ref() {
                    cb(auth_req);
                }
            } else if v
                .get("type")
                .and_then(|v| v.as_str())
                .map(|t| t.contains("list-sync-notification"))
                .unwrap_or(false)
            {
                // V1.1: Handle plaintext list sync notification
                let list_cid = body.get("new_cid").and_then(|v| v.as_str()).unwrap_or("");
                let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let target_did = body.get("entry_did").and_then(|v| v.as_str()).unwrap_or("");
                tracing::info!(
                    "List sync notification (plaintext): cid={}, action={}, target={}",
                    list_cid,
                    action,
                    target_did
                );
            }
        }
    }
}

async fn send_msg(ws: &mut WsStream, msg: String) -> Result<()> {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
        .await?;
    Ok(())
}

async fn read_msg(ws: &mut WsStream) -> Result<String> {
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => return Ok(text.to_string()),
            Ok(_) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!("Connection closed"))
}

/// Read a single text message with a timeout.
/// Returns `Err` on timeout or connection error.
async fn read_msg_with_timeout(ws: &mut WsStream, timeout: Duration) -> Result<String> {
    match tokio::time::timeout(timeout, read_msg(ws)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("Timeout reading message")),
    }
}
