use crate::payment::{AuthResponse, PaymentRequest, PendingAuthStore};

use affinidi_messaging_didcomm::DIDCommAgent;
use affinidi_messaging_didcomm::identity::PrivateIdentity;
use ignite_pay_core::{build_did_document, generate_ignite_did, identity_to_resolved, parse_did_document};
use ignite_pay_core::didcomm::{self, is_jwe};
use ignite_pay_core::identity::{load_did, save_identity};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_tungstenite::connect_async;

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Encapsulates the WebSocket connection to the DIDComm mediator.
pub struct MediatorConnection {
    agent: Arc<Mutex<DIDCommAgent>>,
    identity: PrivateIdentity,
    did_doc: Value,
    our_did: String,
    ws_url: String,
    connected: Arc<Notify>,
    outgoing: Arc<tokio::sync::Mutex<mpsc::UnboundedSender<String>>>,
}

impl std::fmt::Debug for MediatorConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediatorConnection")
            .field("our_did", &self.our_did)
            .field("ws_url", &self.ws_url)
            .finish()
    }
}

impl MediatorConnection {
    /// Create a new mediator connection.
    /// Tries to load a previously saved identity from sled; generates a new one if none found.
    pub fn new(ws_url: &str, db: &sled::Db) -> Result<Self> {
        let (identity, did) = match load_did(db)? {
            Some(saved_did) => {
                tracing::info!("Loaded existing identity: {}", saved_did);
                let id = PrivateIdentity::generate(&saved_did);
                (id, saved_did)
            }
            None => {
                let (id, did) = generate_ignite_did();
                tracing::info!("Generated new identity: {}", did);
                save_identity(db, &id, &did)?;
                (id, did)
            }
        };

        let did_doc = build_did_document(&did, &identity);
        let (agent, _) = didcomm::create_agent(
            PrivateIdentity::generate(&did),
        );

        let (outgoing_tx, _outgoing_rx) = mpsc::unbounded_channel();

        Ok(MediatorConnection {
            agent: Arc::new(Mutex::new(agent)),
            identity,
            did_doc,
            our_did: did,
            ws_url: ws_url.to_string(),
            connected: Arc::new(Notify::new()),
            outgoing: Arc::new(tokio::sync::Mutex::new(outgoing_tx)),
        })
    }

    /// Get our DID string.
    pub fn our_did(&self) -> &str {
        &self.our_did
    }

    /// Connect to mediator, perform plaintext handshake, then start bidirectional loop.
    /// Spawns a background task that handles both sending and receiving.
    pub async fn connect(&self, pending: Arc<PendingAuthStore>) -> Result<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let ws_url = self.ws_url.clone();
        let connected = self.connected.clone();

        // Create a new channel pair for this connection
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();

        // Replace the sender so send_auth_request() uses the new channel
        {
            let mut guard = self.outgoing.lock().await;
            *guard = outgoing_tx;
        }

        tokio::spawn(async move {
            real_ws_client(&ws_url, &agent, &our_did, &did_doc, connected, outgoing_rx, pending).await;
        });

        Ok(())
    }

    /// Send a payment authorization request to the phone via the mediator.
    /// Encrypts and sends the JWE through the WebSocket connection.
    pub async fn send_auth_request(
        &self,
        phone_did: &str,
        payment: &PaymentRequest,
    ) -> Result<String> {
        let msg = didcomm::build_authorization_request(
            &self.our_did,
            phone_did,
            &payment.id,
            &payment.merchant_did,
            payment.amount,
            &payment.description,
        );

        let agent = self.agent.lock().await;
        let jwe = didcomm::pack_encrypted(&agent, &msg, &self.our_did, phone_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        drop(agent);

        // Send the JWE through the outgoing channel
        {
            let sender = self.outgoing.lock().await;
            sender.send(jwe.clone())
                .map_err(|_| anyhow::anyhow!("WebSocket channel closed"))?;
        }

        tracing::info!(
            "Auth request sent for payment {} to phone {}",
            payment.id,
            phone_did
        );

        Ok(jwe)
    }

    /// Register a peer (e.g., the phone) in the DIDComm agent.
    /// Uses the peer's DID to generate a resolved identity for encryption.
    pub async fn add_peer(&self, peer_did: &str) {
        // Generate a resolved identity for the peer so we can encrypt to it.
        // In production, this would come from DID resolution.
        let peer_identity = PrivateIdentity::generate(peer_did);
        let resolved = identity_to_resolved(&peer_identity);
        let mut agent = self.agent.lock().await;
        agent.add_peer(resolved);
    }

    /// Register a peer using a DID document (proper key resolution).
    pub async fn add_peer_from_doc(&self, did: &str, doc: &Value) {
        if let Some(resolved) = parse_did_document(did, doc) {
            let mut agent = self.agent.lock().await;
            agent.add_peer(resolved);
            tracing::info!("Registered peer from DID document: {}", did);
        } else {
            tracing::warn!("Failed to parse DID document for peer: {}", did);
        }
    }
}

/// Reconnecting WebSocket client loop with bidirectional message handling.
async fn real_ws_client(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    _connected: Arc<Notify>,
    mut outgoing_rx: mpsc::UnboundedReceiver<String>,
    pending: Arc<PendingAuthStore>,
) {
    loop {
        match connect_and_run(ws_url, agent, our_did, did_doc, &mut outgoing_rx, &pending).await {
            Ok(()) => {
                tracing::warn!("Mediator disconnected, reconnecting...");
            }
            Err(e) => {
                tracing::error!("WS error: {}, reconnecting in 3s...", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Connect to mediator, perform plaintext handshake, then enter bidirectional loop.
async fn connect_and_run(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    outgoing_rx: &mut mpsc::UnboundedReceiver<String>,
    pending: &PendingAuthStore,
) -> Result<()> {
    let (mut ws, _) = connect_async(ws_url).await?;
    tracing::info!("Connected to mediator: {}", ws_url);

    // --- Phase A: Plaintext handshake ---

    // 1. mediate-request
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;
    tracing::info!("Sent mediate-request (from: {})", our_did);

    // Read mediate-grant
    let grant = read_msg(&mut ws).await?;
    let grant_v: Value = serde_json::from_str(&grant)?;
    if grant_v
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| t.contains("mediate-grant"))
        .unwrap_or(false)
    {
        tracing::info!("Received mediate-grant");
    } else {
        tracing::warn!("Expected mediate-grant, got: {}", grant);
    }

    // 2. keylist-update
    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;
    tracing::info!("Sent keylist-update");

    let kl_resp = read_msg(&mut ws).await?;
    let kl_v: Value = serde_json::from_str(&kl_resp)?;
    if kl_v
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| t.contains("keylist-update"))
        .unwrap_or(false)
    {
        tracing::info!("Received keylist-update-response, registration complete");
    } else {
        tracing::warn!("Expected keylist-update-response, got: {}", kl_resp);
    }

    // 3. peer-introduction — send our DID document
    let intro = didcomm::build_peer_introduction(our_did, did_doc);
    send_msg(&mut ws, serde_json::to_string(&intro)?).await?;
    tracing::info!("Sent peer-introduction (DID doc)");

    tracing::info!("Mediator handshake complete, entering bidirectional loop...");

    // --- Phase B: Bidirectional loop using tokio::select! ---
    loop {
        tokio::select! {
            // Handle incoming messages
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_incoming_message(&text, agent, pending).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }
            // Handle outgoing messages
            jwe = outgoing_rx.recv() => {
                match jwe {
                    Some(msg) => {
                        if let Err(e) = send_msg(&mut ws, msg).await {
                            tracing::error!("Failed to send outgoing message: {}", e);
                            return Err(e);
                        }
                    }
                    None => return Ok(()),
                }
            }
        }
    }
}

/// Handle an incoming message: try JWE unpack, check for auth response.
async fn handle_incoming_message(
    text: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    pending: &PendingAuthStore,
) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg, pending).await;
                return;
            }
            Err(e) => {
                tracing::warn!("JWE unpack failed: {}, trying plaintext", e);
                drop(agent_guard);
            }
        }
    }

    // Plaintext fallback
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse message: {}", e);
            return;
        }
    };

    // Check for auth response in body
    if let Some(body) = v.get("body") {
        if let Some(payment_id) = body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            let list_action = body.get("list_action").and_then(|v| v.as_str()).unwrap_or("none").to_string();
            let merchant_did = body.get("merchant_did").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let response = AuthResponse { authorized, list_action, merchant_did };
            if pending.resolve(payment_id, response) {
                tracing::info!("Resolved pending auth: {} -> {}", payment_id, authorized);
            }
            return;
        }
    }

    tracing::info!(
        "Received non-auth message: {}",
        text.chars().take(100).collect::<String>()
    );
}

/// Process an unpacked DIDComm Message (from JWE or plaintext).
async fn process_inner_message(
    msg: &affinidi_messaging_didcomm::Message,
    pending: &PendingAuthStore,
) {
    // Check for payment-auth-response type
    if msg.typ.contains("payment-auth-response") {
        if let Some(payment_id) = msg.body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = msg.body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            let list_action = msg.body.get("list_action").and_then(|v| v.as_str()).unwrap_or("none").to_string();
            let merchant_did = msg.body.get("merchant_did").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let response = AuthResponse { authorized, list_action, merchant_did };
            if pending.resolve(payment_id, response) {
                tracing::info!("Resolved pending auth (encrypted): {} -> {}", payment_id, authorized);
            }
        }
    } else {
        tracing::info!("Received message type={}, no auth data", msg.typ);
    }
}

async fn send_msg(
    ws: &mut WsStream,
    msg: String,
) -> Result<()> {
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
