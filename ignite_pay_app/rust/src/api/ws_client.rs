use crate::api::auth::{AuthRequest, AuthResponse};
use crate::api::identity::IdentityManager;

use affinidi_messaging_didcomm::DIDCommAgent;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use ignite_pay_core::didcomm::{self, is_jwe};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::connect_async;

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

    /// Connect to the mediator and start the bidirectional loop.
    pub async fn connect(&self, ws_url: &str) -> Result<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let signing_private = self.signing_private;
        let outgoing = self.outgoing.clone();
        let callback = self.pending_auth_callback.clone();
        let ws_url = ws_url.to_string();

        tokio::spawn(async move {
            loop {
                match run_ws_loop(&ws_url, &agent, &our_did, &did_doc, &signing_private, &outgoing, &callback).await {
                    Ok(()) => tracing::warn!("Mediator disconnected, reconnecting..."),
                    Err(e) => tracing::error!("WS error: {}, reconnecting in 3s...", e),
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
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

async fn run_ws_loop(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    signing_private: &[u8; 32],
    outgoing: &Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    callback: &Arc<Mutex<Option<AuthCallback>>>,
) -> Result<()> {
    let (mut ws, _) = connect_async(ws_url).await?;
    tracing::info!("Connected to mediator: {}", ws_url);

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

    // --- Phase A: Plaintext handshake ---
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

    // Set up outgoing channel
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    {
        let mut guard = outgoing.lock().await;
        *guard = Some(out_tx);
    }

    // Bidirectional loop
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_message(&text, agent, callback).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }
            jwe = out_rx.recv() => {
                match jwe {
                    Some(msg) => {
                        send_msg(&mut ws, msg).await?;
                    }
                    None => return Ok(()),
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
