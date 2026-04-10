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

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Callback type for incoming auth requests.
pub type AuthCallback = Box<dyn Fn(AuthRequest) + Send + Sync>;

/// WebSocket client that connects to the DIDComm mediator.
pub struct WsClient {
    agent: Arc<Mutex<DIDCommAgent>>,
    our_did: String,
    did_doc: Value,
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    pending_auth_callback: Arc<Mutex<Option<AuthCallback>>>,
}

impl WsClient {
    pub fn new(identity_mgr: &IdentityManager) -> Self {
        Self {
            agent: identity_mgr.agent(),
            our_did: identity_mgr.did().to_string(),
            did_doc: identity_mgr.did_doc().clone(),
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
        let outgoing = self.outgoing.clone();
        let callback = self.pending_auth_callback.clone();
        let ws_url = ws_url.to_string();

        tokio::spawn(async move {
            loop {
                match run_ws_loop(&ws_url, &agent, &our_did, &did_doc, &outgoing, &callback).await {
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
        let msg = didcomm::build_authorization_response(
            &self.our_did,
            mcp_did,
            &response.payment_id,
            response.authorized,
            &response.list_action,
        );

        let jwe = {
            let agent = self.agent.lock().await;
            didcomm::pack_encrypted(&agent, &msg, &self.our_did, mcp_did)
                .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?
        };

        let outgoing_guard = self.outgoing.lock().await;
        if let Some(sender) = outgoing_guard.as_ref() {
            sender.send(jwe).map_err(|_| anyhow::anyhow!("WS channel closed"))?;
        }

        Ok(())
    }

    /// Register a peer DID in the agent for encryption.
    pub async fn add_peer(&self, peer_did: &str) {
        let peer_identity = affinidi_messaging_didcomm::identity::PrivateIdentity::generate(peer_did);
        let resolved = ignite_pay_core::identity_to_resolved(&peer_identity);
        let mut agent = self.agent.lock().await;
        agent.add_peer(resolved);
    }
}

async fn run_ws_loop(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    outgoing: &Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    callback: &Arc<Mutex<Option<AuthCallback>>>,
) -> Result<()> {
    let (mut ws, _) = connect_async(ws_url).await?;
    tracing::info!("Connected to mediator: {}", ws_url);

    // Handshake
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;

    let grant = read_msg(&mut ws).await?;
    let grant_v: Value = serde_json::from_str(&grant)?;
    if !grant_v.get("type").and_then(|v| v.as_str()).map(|t| t.contains("mediate-grant")).unwrap_or(false) {
        tracing::warn!("Expected mediate-grant, got: {}", grant);
    }

    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;

    let kl_resp = read_msg(&mut ws).await?;
    let kl_v: Value = serde_json::from_str(&kl_resp)?;
    if !kl_v.get("type").and_then(|v| v.as_str()).map(|t| t.contains("keylist-update")).unwrap_or(false) {
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
                            description: msg.body.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        };
                        let cb_guard = callback.lock().await;
                        if let Some(cb) = cb_guard.as_ref() {
                            cb(auth_req);
                        }
                    }
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
                    payment_id: body.get("payment_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    merchant_did: body.get("merchant_did").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    amount: body.get("amount").and_then(|v| v.as_u64()).unwrap_or(0),
                    description: body.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                };
                let cb_guard = callback.lock().await;
                if let Some(cb) = cb_guard.as_ref() {
                    cb(auth_req);
                }
            }
        }
    }
}

async fn send_msg(ws: &mut WsStream, msg: String) -> Result<()> {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.into())).await?;
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
