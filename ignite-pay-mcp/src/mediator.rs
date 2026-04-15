use crate::payment::{AuthResponse, PaymentRequest, PendingAuthStore};

use affinidi_messaging_didcomm::identity::PrivateIdentity;
use base64::Engine;
use affinidi_messaging_didcomm::DIDCommAgent;
use ignite_pay_core::didcomm::{self, is_jwe};
use ignite_pay_core::identity::{load_identity, save_identity};
use ignite_pay_core::{
    build_did_document, generate_ignite_did, identity_to_resolved, parse_did_document,
};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_tungstenite::connect_async;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Encapsulates the WebSocket connection to the DIDComm mediator.
pub struct MediatorConnection {
    agent: Arc<Mutex<DIDCommAgent>>,
    did_doc: Value,
    our_did: String,
    ws_url: String,
    connected: Arc<Notify>,
    outgoing: Arc<tokio::sync::Mutex<mpsc::UnboundedSender<String>>>,
    /// DID of the paired phone (set during connection-request handshake).
    paired_phone: Arc<tokio::sync::Mutex<Option<String>>>,
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
        let (identity, did) = match load_identity(db)? {
            Some(loaded) => {
                let did = loaded.did.clone();
                tracing::info!("Loaded existing identity: {}", did);
                (loaded, did)
            }
            None => {
                let (id, did) = generate_ignite_did();
                tracing::info!("Generated new identity: {}", did);
                save_identity(db, &id, &did)?;
                (id, did)
            }
        };

        let did_doc = build_did_document(&did, &identity);
        let (agent, _) = didcomm::create_agent(identity);

        let (outgoing_tx, _outgoing_rx) = mpsc::unbounded_channel();

        Ok(MediatorConnection {
            agent: Arc::new(Mutex::new(agent)),
            did_doc,
            our_did: did,
            ws_url: ws_url.to_string(),
            connected: Arc::new(Notify::new()),
            outgoing: Arc::new(tokio::sync::Mutex::new(outgoing_tx)),
            paired_phone: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// Get our DID string.
    pub fn our_did(&self) -> &str {
        &self.our_did
    }

    /// Get the DID document JSON.
    pub fn did_doc(&self) -> &Value {
        &self.did_doc
    }

    /// Get the mediator WS URL.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Get the paired phone DID (if any phone has completed pairing).
    pub async fn paired_phone_did(&self) -> Option<String> {
        self.paired_phone.lock().await.clone()
    }

    /// Generate an Out-of-Band invitation URL for P2P pairing.
    /// The phone scans this QR code to learn our DID, DID document, and mediator endpoint.
    /// Returns the OOB invitation as a URL-encoded JSON string.
    pub fn generate_invitation(&self) -> String {
        let invitation = didcomm::build_oob_invitation(
            &self.our_did,
            "Ignite Pay MCP",
            &self.ws_url,
            &self.did_doc,
        );
        // Serialize the invitation message to JSON, then base64url-encode it
        // as a DIDComm out-of-band URL: didcomm://?_oob=<base64url-json>
        let json = serde_json::to_string(&invitation).unwrap_or_default();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes());
        format!("didcomm://?_oob={}", b64)
    }

    /// Generate an ASCII QR code for the OOB invitation.
    pub fn generate_invitation_qr(&self) -> Result<String> {
        let url = self.generate_invitation();
        let code = qrcode::QrCode::new(url.as_bytes())
            .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
        let string = code
            .render::<char>()
            .quiet_zone(false)
            .module_dimensions(2, 1)
            .build();
        Ok(string)
    }

    /// Connect to mediator, perform plaintext handshake, then start bidirectional loop.
    /// Spawns a background task that handles both sending and receiving.
    pub async fn connect(&self, pending: Arc<PendingAuthStore>) -> Result<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let ws_url = self.ws_url.clone();
        let connected = self.connected.clone();
        let paired_phone = self.paired_phone.clone();

        // Create a new channel pair for this connection
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();

        // Replace the sender so send_auth_request() uses the new channel
        {
            let mut guard = self.outgoing.lock().await;
            *guard = outgoing_tx;
        }

        tokio::spawn(async move {
            real_ws_client(
                &ws_url,
                &agent,
                &our_did,
                &did_doc,
                connected,
                outgoing_rx,
                pending,
                paired_phone,
            )
            .await;
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
            sender
                .send(jwe.clone())
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

    /// Send a list-sync notification to the phone after list changes (V1.1).
    pub async fn send_list_sync_notification(
        &self,
        phone_did: &str,
        list_type: &str,
        action: &str,
        entry_did: &str,
        new_cid: &str,
    ) -> Result<String> {
        let msg = didcomm::build_list_sync_notification(
            &self.our_did,
            phone_did,
            list_type,
            action,
            entry_did,
            new_cid,
        );

        let agent = self.agent.lock().await;
        let jwe = didcomm::pack_encrypted(&agent, &msg, &self.our_did, phone_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        drop(agent);

        {
            let sender = self.outgoing.lock().await;
            sender
                .send(jwe.clone())
                .map_err(|_| anyhow::anyhow!("WebSocket channel closed"))?;
        }

        tracing::info!(
            "List sync notification sent: {} {} for {} (cid={})",
            action,
            list_type,
            entry_did,
            new_cid
        );

        Ok(jwe)
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
    paired_phone: Arc<tokio::sync::Mutex<Option<String>>>,
) {
    loop {
        match connect_and_run(
            ws_url,
            agent,
            our_did,
            did_doc,
            &mut outgoing_rx,
            &pending,
            &paired_phone,
        )
        .await
        {
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
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
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
    let mediator_did = challenge["from"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing from in challenge"))?
        .to_string();

    // Register mediator as peer so we can encrypt to them
    let mediator_doc = &challenge["body"]["did_document"];
    {
        let mut agent_guard = agent.lock().await;
        if let Some(resolved) = parse_did_document(&mediator_did, mediator_doc) {
            agent_guard.add_peer(resolved);
            tracing::info!("Registered mediator as peer: {}", mediator_did);
        }
    }

    // Build and send encrypted challenge-response
    let response_msg = didcomm::build_ws_challenge_response(our_did, &mediator_did, nonce, did_doc);
    {
        let agent_guard = agent.lock().await;
        let jwe = didcomm::pack_encrypted(&agent_guard, &response_msg, our_did, &mediator_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        send_msg(&mut ws, jwe).await?;
        tracing::info!("Sent WS challenge-response (encrypted)");
    }

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
            let reason = auth_v["body"]["reason"]
                .as_str()
                .unwrap_or("unknown");
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

    tracing::info!("Mediator handshake complete, checking for queued messages...");

    // --- Phase A2: Message Pickup 3.0 — pull offline messages ---

    // Send status-request to learn how many messages are queued
    let sr = didcomm::build_status_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&sr)?).await?;
    tracing::info!("Sent status-request");

    // Read status response
    let status_text = read_msg_with_timeout(&mut ws, Duration::from_secs(5)).await;
    match status_text {
        Ok(text) => {
            let sv: Value = serde_json::from_str(&text).unwrap_or_default();
            if sv.get("type")
                .and_then(|v| v.as_str())
                .map(|t| t.contains("status"))
                .unwrap_or(false)
            {
                let count = sv
                    .get("body")
                    .and_then(|b| b.get("message_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                tracing::info!("Mediator reports {} queued message(s)", count);

                if count > 0 {
                    // Request batch delivery of all queued messages
                    let bp = didcomm::build_batch_pickup(our_did, count as usize);
                    send_msg(&mut ws, serde_json::to_string(&bp)?).await?;
                    tracing::info!("Sent batch-pickup (count: {})", count);

                    // Read the batch response
                    let batch_text = read_msg_with_timeout(&mut ws, Duration::from_secs(10)).await;
                    match batch_text {
                        Ok(batch_str) => {
                            let bv: Value = serde_json::from_str(&batch_str).unwrap_or_default();
                            if bv.get("type")
                                .and_then(|v| v.as_str())
                                .map(|t| t.contains("batch"))
                                .unwrap_or(false)
                            {
                                if let Some(messages) = bv
                                    .get("body")
                                    .and_then(|b| b.get("messages"))
                                    .and_then(|v| v.as_array())
                                {
                                    tracing::info!(
                                        "Received batch of {} queued message(s), processing...",
                                        messages.len()
                                    );
                                    for entry in messages {
                                        if let Some(jwe) = entry.get("message").and_then(|m| m.as_str()) {
                                            handle_incoming_message(
                                                jwe, agent, pending, paired_phone,
                                            )
                                            .await;
                                        }
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    "Expected batch response, got: {}",
                                    batch_str.chars().take(120).collect::<String>()
                                );
                            }
                        }
                        Err(_) => {
                            tracing::warn!("Timeout waiting for batch response, proceeding to main loop");
                        }
                    }
                }
            } else {
                tracing::debug!(
                    "Non-status response during pickup phase: {}",
                    text.chars().take(120).collect::<String>()
                );
            }
        }
        Err(_) => {
            tracing::warn!("Timeout waiting for status response, proceeding to main loop");
        }
    }

    tracing::info!("Entering bidirectional loop...");

    // --- Phase B: Bidirectional loop using tokio::select! ---
    loop {
        tokio::select! {
            // Handle incoming messages
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_incoming_message(&text, agent, pending, paired_phone).await;
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

/// Handle an incoming message: try JWE unpack, check for auth response or connection request.
async fn handle_incoming_message(
    text: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    pending: &PendingAuthStore,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg, pending, paired_phone, agent).await;
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

    // Check for connection-request in plaintext
    if v
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t.contains("connection-request"))
        .unwrap_or(false)
    {
        let phone_did = v["from"].as_str().unwrap_or("");
        let push_channel = v["body"]
            .get("push_channel")
            .and_then(|v| v.as_str())
            .unwrap_or("fcm");

        tracing::info!(
            "Received plaintext connection-request from phone: {} (push_channel: {})",
            phone_did,
            push_channel
        );

        // Try to parse phone's DID document from the message body
        if let Some(phone_doc) = v["body"].get("did_document") {
            if let Some(resolved) = parse_did_document(phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered phone peer from DID document: {}", phone_did);
            }
        }

        // Store the phone DID
        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.to_string());
        }

        tracing::info!(
            "Phone {} paired successfully via plaintext (push_channel: {})",
            phone_did,
            push_channel
        );
        return;
    }

    // Check for auth response in body
    if let Some(body) = v.get("body") {
        if let Some(payment_id) = body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = body
                .get("authorized")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let list_action = body
                .get("list_action")
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string();
            let merchant_did = body
                .get("merchant_did")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let session_key_pubkey = body
                .get("session_key_pubkey")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_key_secret_key = body
                .get("session_key_secret_key")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_key_tx_signature = body
                .get("session_key_tx_signature")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_expires_at = body.get("session_expires_at").and_then(|v| v.as_i64());
            let spending_limit = body.get("spending_limit").and_then(|v| v.as_u64());
            let scopes = body.get("scopes").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
            let list_label = body
                .get("list_label")
                .and_then(|v| v.as_str())
                .map(String::from);
            let list_max_amount = body.get("list_max_amount").and_then(|v| v.as_u64());
            let response = AuthResponse {
                authorized,
                list_action,
                merchant_did,
                session_key_pubkey,
                session_key_secret_key,
                session_key_tx_signature,
                session_expires_at,
                spending_limit,
                scopes,
                list_label,
                list_max_amount,
            };
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

/// Process an unpacked DIDComm Message (from JWE).
async fn process_inner_message(
    msg: &affinidi_messaging_didcomm::Message,
    pending: &PendingAuthStore,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    agent: &Arc<Mutex<DIDCommAgent>>,
) {
    // Check for connection-request type (pairing from phone)
    if msg.typ.contains("connection-request") {
        let phone_did = msg
            .from
            .clone()
            .unwrap_or_default();
        let push_channel = msg
            .body
            .get("push_channel")
            .and_then(|v| v.as_str())
            .unwrap_or("fcm");

        tracing::info!(
            "Received connection-request from phone: {} (push_channel: {})",
            phone_did,
            push_channel
        );

        // Try to register phone from its DID document included in the message body
        if let Some(phone_doc) = msg.body.get("did_document") {
            if let Some(resolved) = parse_did_document(&phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered phone peer from DID document: {}", phone_did);
            }
        } else {
            tracing::warn!(
                "No did_document in connection-request from {}, encryption may fail",
                phone_did
            );
        }

        // Store the phone DID
        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.clone());
        }

        tracing::info!(
            "Phone {} paired successfully (push_channel: {})",
            phone_did,
            push_channel
        );
        return;
    }

    // Check for payment-auth-response type
    if msg.typ.contains("payment-auth-response") {
        if let Some(payment_id) = msg.body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = msg
                .body
                .get("authorized")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let list_action = msg
                .body
                .get("list_action")
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string();
            let merchant_did = msg
                .body
                .get("merchant_did")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let session_key_pubkey = msg
                .body
                .get("session_key_pubkey")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_key_secret_key = msg
                .body
                .get("session_key_secret_key")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_key_tx_signature = msg
                .body
                .get("session_key_tx_signature")
                .and_then(|v| v.as_str())
                .map(String::from);
            let session_expires_at = msg.body.get("session_expires_at").and_then(|v| v.as_i64());
            let spending_limit = msg.body.get("spending_limit").and_then(|v| v.as_u64());
            let scopes = msg
                .body
                .get("scopes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                });
            let list_label = msg
                .body
                .get("list_label")
                .and_then(|v| v.as_str())
                .map(String::from);
            let list_max_amount = msg.body.get("list_max_amount").and_then(|v| v.as_u64());
            let response = AuthResponse {
                authorized,
                list_action,
                merchant_did,
                session_key_pubkey,
                session_key_secret_key,
                session_key_tx_signature,
                session_expires_at,
                spending_limit,
                scopes,
                list_label,
                list_max_amount,
            };
            if pending.resolve(payment_id, response) {
                tracing::info!(
                    "Resolved pending auth (encrypted): {} -> {}",
                    payment_id,
                    authorized
                );
            }
        }
    } else {
        tracing::info!("Received message type={}, no auth data", msg.typ);
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
