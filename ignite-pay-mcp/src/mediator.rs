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

/// Command sent when a create-channel-request is received from the phone app.
pub struct CreateChannelCommand {
    pub requestor_did: String,
    pub hub_endpoint: String,
    pub provider_pubkey: String,
    pub token_mint: String,
    pub deposit: u64,
    pub tree_depth: u32,
}

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
    /// DID of a phone that has sent connection-request but not yet confirmed (pending 3-step handshake).
    pending_phone: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Mediator HTTP URL of the paired phone (used to forward messages via HTTP POST).
    phone_mediator_http_url: Arc<tokio::sync::Mutex<Option<String>>>,
    signing_private: [u8; 32],
    db: sled::Db,
}

fn save_paired_phone(db: &sled::Db, did: &str) {
    let _ = db.insert("__paired_phone__", did.as_bytes());
    let _ = db.flush();
}

fn load_paired_phone(db: &sled::Db) -> Option<String> {
    db.get("__paired_phone__")
        .ok()
        .flatten()
        .map(|v| String::from_utf8_lossy(&v).to_string())
}

fn save_phone_mediator_http_url(db: &sled::Db, url: &str) {
    let _ = db.insert("__phone_mediator_http_url__", url.as_bytes());
    let _ = db.flush();
}

fn load_phone_mediator_http_url(db: &sled::Db) -> Option<String> {
    db.get("__phone_mediator_http_url__")
        .ok()
        .flatten()
        .map(|v| String::from_utf8_lossy(&v).to_string())
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
        let signing_private = identity.signing_private
            .ok_or_else(|| anyhow::anyhow!("no signing key in identity"))?;
        let (agent, _) = didcomm::create_agent(identity);

        let (outgoing_tx, _outgoing_rx) = mpsc::unbounded_channel();

        Ok(MediatorConnection {
            agent: Arc::new(Mutex::new(agent)),
            did_doc,
            our_did: did,
            ws_url: ws_url.to_string(),
            connected: Arc::new(Notify::new()),
            outgoing: Arc::new(tokio::sync::Mutex::new(outgoing_tx)),
            paired_phone: Arc::new(tokio::sync::Mutex::new(load_paired_phone(db))),
            pending_phone: Arc::new(tokio::sync::Mutex::new(None)),
            phone_mediator_http_url: Arc::new(tokio::sync::Mutex::new(load_phone_mediator_http_url(db))),
            signing_private,
            db: db.clone(),
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

    /// Generate a lightweight Out-of-Band invitation URL without the DID document.
    /// The DID document is resolved later by the phone via the mediator's peer introduction.
    /// This produces a much shorter QR code that is scannable by phone cameras.
    pub fn generate_invitation(&self) -> String {
        // Minimal invitation with routing info for message delivery.
        // routing_keys = our DID so the sender knows how to route to us.
        // Use HTTP URL directly so the phone app doesn't need to convert.
        let endpoint = self.ws_url
            .replace("wss://", "https://")
            .replace("ws://", "http://")
            .trim_end_matches("/ws")
            .to_string();
        let invitation = serde_json::json!({
            "type": "https://didcomm.org/out-of-band/2.0/invitation",
            "from": self.our_did,
            "body": {
                "services": [{
                    "service_endpoint": endpoint,
                    "routing_keys": [self.our_did]
                }]
            }
        });
        let json = serde_json::to_string(&invitation).unwrap_or_default();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes());
        format!("didcomm://?_oob={}", b64)
    }

    /// Generate a full Out-of-Band invitation URL including the DID document.
    pub fn generate_invitation_full(&self) -> String {
        let invitation = didcomm::build_oob_invitation(
            &self.our_did,
            "Ignite Pay MCP",
            &self.ws_url,
            &self.did_doc,
        );
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

    /// Generate the pairing QR as an SVG file and save to disk.
    pub fn generate_invitation_qr_svg(&self, path: &str) -> Result<String> {
        let url = self.generate_invitation();
        let code = qrcode::QrCode::new(url.as_bytes())
            .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
        let svg = code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(1024, 1024)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .quiet_zone(true)
            .build();
        std::fs::write(path, &svg)?;
        Ok(url)
    }

    /// Connect to mediator, perform plaintext handshake, then start bidirectional loop.
    /// Spawns a background task that handles both sending and receiving.
    pub async fn connect(
        &self,
        pending: Arc<PendingAuthStore>,
        create_channel_tx: Option<mpsc::UnboundedSender<CreateChannelCommand>>,
    ) -> Result<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let ws_url = self.ws_url.clone();
        let connected = self.connected.clone();
        let paired_phone = self.paired_phone.clone();
        let pending_phone = self.pending_phone.clone();
        let phone_mediator_http_url = self.phone_mediator_http_url.clone();
        let signing_private = self.signing_private;
        let db = self.db.clone();

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
                pending_phone,
                phone_mediator_http_url,
                create_channel_tx,
                &signing_private,
                &db,
            )
            .await;
        });

        Ok(())
    }

    /// Send a payment authorization request to the phone.
    /// Encrypts to JWE, wraps in forward message, and sends directly to the phone's mediator.
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

        self.send_to_phone_mediator(phone_did, &jwe).await?;

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

        self.send_to_phone_mediator(phone_did, &jwe).await?;

        tracing::info!(
            "List sync notification sent: {} {} for {} (cid={})",
            action,
            list_type,
            entry_did,
            new_cid
        );

        Ok(jwe)
    }

    /// Send a create-channel response back to the requesting app.
    pub async fn send_create_channel_response(
        &self,
        app_did: &str,
        channel_id: &str,
        sequence: u64,
        current_root: &str,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<String> {
        let msg = didcomm::build_create_channel_response(
            &self.our_did,
            app_did,
            channel_id,
            sequence,
            current_root,
            success,
            error_message,
        );

        let agent = self.agent.lock().await;
        let jwe = didcomm::pack_encrypted(&agent, &msg, &self.our_did, app_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        drop(agent);

        self.send_to_phone_mediator(app_did, &jwe).await?;

        tracing::info!(
            "Create channel response sent to {}: success={}, channel_id={}",
            app_did,
            success,
            channel_id
        );

        Ok(jwe)
    }

    /// Send a JWE to the phone's mediator via HTTP POST.
    /// Wraps the JWE in a forward message so the mediator routes it to the phone.
    /// Uses the mediator's public POST / endpoint (no auth required).
    async fn send_to_phone_mediator(&self, phone_did: &str, jwe: &str) -> Result<()> {
        let phone_http_url = self.phone_mediator_http_url.lock().await.clone();

        // If same mediator, send through our own outgoing channel
        let same_mediator = match &phone_http_url {
            Some(url) => {
                // Derive our own HTTP URL from WS URL for comparison
                let our_http = self.ws_url
                    .replace("wss://", "https://")
                    .replace("ws://", "http://")
                    .trim_end_matches("/ws")
                    .to_string() + "/";
                url == &our_http
            }
            None => false,
        };

        if same_mediator {
            let sender = self.outgoing.lock().await;
            sender
                .send(jwe.to_string())
                .map_err(|_| anyhow::anyhow!("WebSocket channel closed"))?;
            return Ok(());
        }

        // Different mediator — wrap in forward and send via HTTP POST
        let http_url = phone_http_url
            .ok_or_else(|| anyhow::anyhow!("Phone mediator HTTP URL not known (not paired?)"))?;

        let forward_msg = serde_json::json!({
            "type": "https://didcomm.org/routing/2.0/forward",
            "id": format!("fwd-{}", uuid::Uuid::new_v4()),
            "body": { "next": phone_did },
            "attachments": [{
                "data": { "json": serde_json::Value::String(jwe.to_string()) }
            }]
        });

        let forward_str = serde_json::to_string(&forward_msg)?;

        tracing::info!(
            "Sending forward-wrapped JWE to phone {} via their mediator HTTP {}",
            phone_did,
            http_url
        );

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
                "Phone mediator rejected message: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }
}
async fn real_ws_client(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    _connected: Arc<Notify>,
    mut outgoing_rx: mpsc::UnboundedReceiver<String>,
    pending: Arc<PendingAuthStore>,
    paired_phone: Arc<tokio::sync::Mutex<Option<String>>>,
    pending_phone: Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: Arc<tokio::sync::Mutex<Option<String>>>,
    create_channel_tx: Option<mpsc::UnboundedSender<CreateChannelCommand>>,
    signing_private: &[u8; 32],
    db: &sled::Db,
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
            &pending_phone,
            &phone_mediator_http_url,
            &create_channel_tx,
            signing_private,
            db,
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
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    create_channel_tx: &Option<mpsc::UnboundedSender<CreateChannelCommand>>,
    signing_private: &[u8; 32],
    db: &sled::Db,
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
                                                jwe, agent, pending, paired_phone, pending_phone, phone_mediator_http_url, create_channel_tx, db, our_did, did_doc, ws_url, signing_private,
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
                        handle_incoming_message(&text, agent, pending, paired_phone, pending_phone, phone_mediator_http_url, create_channel_tx, db, our_did, did_doc, ws_url, signing_private).await;
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

/// Forward a message to a phone's mediator via HTTP POST.
async fn http_forward_to_phone(phone_did: &str, inner_msg: &str, phone_http_url: &str) -> Result<()> {
    let forward_msg = serde_json::json!({
        "type": "https://didcomm.org/routing/2.0/forward",
        "id": format!("fwd-{}", uuid::Uuid::new_v4()),
        "body": { "next": phone_did },
        "attachments": [{
            "data": { "json": serde_json::from_str::<serde_json::Value>(inner_msg).unwrap_or_else(|_| serde_json::Value::String(inner_msg.to_string())) }
        }]
    });
    let forward_str = serde_json::to_string(&forward_msg)?;
    tracing::info!(
        "Sending connection-response to phone {} via mediator HTTP {}",
        phone_did, phone_http_url
    );
    let client = reqwest::Client::new();
    let resp = client
        .post(phone_http_url)
        .header("Content-Type", "application/json")
        .body(forward_str)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Phone mediator rejected: {} - {}", status, body));
    }
    Ok(())
}

/// Build and send a connection-response to the phone via its mediator.
/// Includes MCP's nonce and Ed25519 signature so the phone can verify MCP's identity.
async fn send_conn_response(
    phone_did: &str,
    phone_http_url: &str,
    our_did: &str,
    did_doc: &Value,
    our_ws_url: &str,
    signing_private: &[u8; 32],
    accepted: bool,
) {
    // Derive our HTTP URL from our WS URL for inclusion in the response
    let our_http_url = our_ws_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches("/ws")
        .to_string() + "/";

    let body = if accepted {
        let nonce = uuid::Uuid::new_v4().to_string();
        let signature = ignite_pay_core::sign_message(signing_private, nonce.as_bytes());
        serde_json::json!({
            "accepted": true,
            "did_document": did_doc,
            "mediator_http_url": our_http_url,
            "mcp_nonce": nonce,
            "mcp_signature": signature,
        })
    } else {
        serde_json::json!({ "accepted": false })
    };
    let msg = serde_json::json!({
        "type": "https://didcomm.org/ignite-pay/1.0/connection-response",
        "id": format!("conn-resp-{}", uuid::Uuid::new_v4()),
        "from": our_did,
        "to": [phone_did],
        "body": body,
    });
    let msg_str = serde_json::to_string(&msg).unwrap_or_default();
    match http_forward_to_phone(phone_did, &msg_str, phone_http_url).await {
        Ok(()) => tracing::info!("Sent connection-response to {} (accepted: {})", phone_did, accepted),
        Err(e) => tracing::error!("Failed to send connection-response: {}", e),
    }
}

/// Handle an incoming message: try JWE unpack, check for auth response or connection request.
async fn handle_incoming_message(
    text: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    pending: &PendingAuthStore,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    create_channel_tx: &Option<mpsc::UnboundedSender<CreateChannelCommand>>,
    db: &sled::Db,
    our_did: &str,
    did_doc: &Value,
    mcp_ws_url: &str,
    signing_private: &[u8; 32],
) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg, pending, paired_phone, pending_phone, phone_mediator_http_url, agent, create_channel_tx, db, our_did, did_doc, mcp_ws_url, signing_private).await;
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
        let phone_http_url = v["body"]
            .get("mediator_http_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        tracing::info!(
            "Received plaintext connection-request from phone: {} (push_channel: {}, mediator: {:?})",
            phone_did,
            push_channel,
            phone_http_url
        );

        // Check if already paired — only first-time pairing is allowed
        {
            let guard = paired_phone.lock().await;
            if guard.is_some() {
                tracing::warn!("Rejecting pairing from {}: already paired", phone_did);
                drop(guard);
                if let Some(ref http_url) = phone_http_url {
                    send_conn_response(phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, false).await;
                }
                return;
            }
        }

        // Allow overwriting a stale pending pairing (e.g. phone reinstalled, new DID)
        {
            let mut guard = pending_phone.lock().await;
            if let Some(ref existing) = *guard {
                tracing::warn!("Overwriting pending pairing from {} with new request from {}", existing, phone_did);
            }
            *guard = None;
        }

        // Try to parse phone's DID document from the message body
        if let Some(phone_doc) = v["body"].get("did_document") {
            if let Some(resolved) = parse_did_document(phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered phone peer from DID document: {}", phone_did);
            }
        }

        // Store as pending (not yet fully paired — needs connection-confirm)
        {
            let mut guard = pending_phone.lock().await;
            *guard = Some(phone_did.to_string());
        }

        // Store the phone's mediator HTTP URL
        if let Some(ref http_url) = phone_http_url {
            let mut guard = phone_mediator_http_url.lock().await;
            *guard = Some(http_url.clone());
            save_phone_mediator_http_url(db, http_url);
            tracing::info!("Saved phone mediator HTTP URL: {}", http_url);
        }

        tracing::info!(
            "Phone {} connection-request stored as pending (push_channel: {})",
            phone_did,
            push_channel
        );

        // Send connection-response back to phone with MCP's identity info
        if let Some(ref http_url) = phone_http_url {
            send_conn_response(phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, true).await;
        } else {
            tracing::warn!("Cannot send connection-response: phone mediator HTTP URL missing");
        }

        return;
    }

    // Check for connection-confirm in plaintext (3-step handshake step 3)
    if v
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t.contains("connection-confirm"))
        .unwrap_or(false)
    {
        let phone_did = v["from"].as_str().unwrap_or("");
        let phone_nonce = v["body"]
            .get("phone_nonce")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let phone_signature = v["body"]
            .get("phone_signature")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tracing::info!(
            "Received plaintext connection-confirm from phone: {} (nonce: {}...)",
            phone_did,
            &phone_nonce[..phone_nonce.len().min(8)]
        );

        // Verify that this phone has a pending pairing
        {
            let guard = pending_phone.lock().await;
            match guard.as_deref() {
                Some(did) if did == phone_did => {}
                _ => {
                    tracing::warn!("No pending pairing for {}, ignoring connection-confirm", phone_did);
                    return;
                }
            }
        }

        // Verify phone's signature over the nonce
        let sig_valid = ignite_pay_core::verify_did_signature(phone_did, phone_nonce, phone_signature);
        if !sig_valid {
            tracing::warn!("Phone {} signature verification FAILED, rejecting", phone_did);
            let mut guard = pending_phone.lock().await;
            *guard = None;
            return;
        }

        tracing::info!("Phone {} signature verified, completing pairing", phone_did);

        // Move from pending to paired
        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.to_string());
        }
        {
            let mut guard = pending_phone.lock().await;
            *guard = None;
        }
        save_paired_phone(db, phone_did);

        tracing::info!("Phone {} fully paired", phone_did);
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
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    agent: &Arc<Mutex<DIDCommAgent>>,
    create_channel_tx: &Option<mpsc::UnboundedSender<CreateChannelCommand>>,
    db: &sled::Db,
    our_did: &str,
    did_doc: &Value,
    mcp_ws_url: &str,
    signing_private: &[u8; 32],
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
        let phone_http_url = msg
            .body
            .get("mediator_http_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        tracing::info!(
            "Received connection-request from phone: {} (push_channel: {}, mediator: {:?})",
            phone_did,
            push_channel,
            phone_http_url
        );

        // Check if already paired — only first-time pairing is allowed
        {
            let guard = paired_phone.lock().await;
            if guard.is_some() {
                tracing::warn!("Rejecting pairing from {}: already paired", phone_did);
                drop(guard);
                if let Some(ref http_url) = phone_http_url {
                    send_conn_response(&phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, false).await;
                }
                return;
            }
        }

        // Allow overwriting a stale pending pairing (e.g. phone reinstalled, new DID)
        {
            let mut guard = pending_phone.lock().await;
            if let Some(ref existing) = *guard {
                tracing::warn!("Overwriting pending pairing from {} with new request from {}", existing, phone_did);
            }
            *guard = None;
        }

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

        // Store as pending (not yet fully paired — needs connection-confirm)
        {
            let mut guard = pending_phone.lock().await;
            *guard = Some(phone_did.clone());
        }

        // Store the phone's mediator HTTP URL
        if let Some(ref http_url) = phone_http_url {
            let mut guard = phone_mediator_http_url.lock().await;
            *guard = Some(http_url.clone());
            save_phone_mediator_http_url(db, http_url);
            tracing::info!("Saved phone mediator HTTP URL: {}", http_url);
        }

        tracing::info!(
            "Phone {} connection-request stored as pending (push_channel: {})",
            phone_did,
            push_channel
        );

        // Send connection-response back to phone with MCP's identity info
        if let Some(ref http_url) = phone_http_url {
            send_conn_response(&phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, true).await;
        } else {
            tracing::warn!("Cannot send connection-response: phone mediator HTTP URL missing");
        }

        return;
    }

    // Check for connection-confirm type (3-step handshake step 3)
    if msg.typ.contains("connection-confirm") {
        let phone_did = msg.from.clone().unwrap_or_default();
        let phone_nonce = msg
            .body
            .get("phone_nonce")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let phone_signature = msg
            .body
            .get("phone_signature")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tracing::info!(
            "Received connection-confirm from phone: {} (nonce: {}...)",
            phone_did,
            &phone_nonce[..phone_nonce.len().min(8)]
        );

        // Verify that this phone has a pending pairing
        {
            let guard = pending_phone.lock().await;
            match guard.as_deref() {
                Some(did) if did == phone_did => {}
                _ => {
                    tracing::warn!("No pending pairing for {}, ignoring connection-confirm", phone_did);
                    return;
                }
            }
        }

        // Verify phone's signature over the nonce
        let sig_valid = ignite_pay_core::verify_did_signature(&phone_did, phone_nonce, phone_signature);
        if !sig_valid {
            tracing::warn!("Phone {} signature verification FAILED, rejecting", phone_did);
            let mut guard = pending_phone.lock().await;
            *guard = None;
            return;
        }

        tracing::info!("Phone {} signature verified, completing pairing", phone_did);

        // Move from pending to paired
        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.clone());
        }
        {
            let mut guard = pending_phone.lock().await;
            *guard = None;
        }
        save_paired_phone(db, &phone_did);

        tracing::info!("Phone {} fully paired", phone_did);
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
    } else if msg.typ.contains("create-channel-request") {
        let requestor_did = msg.from.clone().unwrap_or_default();
        let hub_endpoint = msg
            .body
            .get("hub_endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let provider_pubkey = msg
            .body
            .get("provider_pubkey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let token_mint = msg
            .body
            .get("token_mint")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let deposit = msg.body.get("deposit").and_then(|v| v.as_u64()).unwrap_or(0);
        let tree_depth = msg
            .body
            .get("tree_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as u32;

        tracing::info!(
            "Received create-channel-request from {}: hub={}",
            requestor_did,
            hub_endpoint
        );

        if let Some(tx) = create_channel_tx {
            let cmd = CreateChannelCommand {
                requestor_did,
                hub_endpoint,
                provider_pubkey,
                token_mint,
                deposit,
                tree_depth,
            };
            if let Err(e) = tx.send(cmd) {
                tracing::error!("Failed to send CreateChannelCommand: {}", e);
            }
        } else {
            tracing::warn!("No create_channel_tx available, ignoring create-channel-request");
        }
    } else {
        tracing::info!("Received message type={}, no handler", msg.typ);
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
