use anyhow::Result;
use affinidi_messaging_didcomm::DIDCommAgent;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use ignite_pay_core::didcomm;
use ignite_pay_core::didcomm::is_jwe;
use ignite_pay_core::identity::{load_identity, save_identity};
use ignite_pay_core::{build_did_document, generate_ignite_did, parse_did_document};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_tungstenite::connect_async;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Command sent when a create-channel-request is received from the merchant app.
pub struct CreateChannelCommand {
    pub requestor_did: String,
    pub hub_endpoint: String,
    pub provider_pubkey: String,
    pub token_mint: String,
    pub deposit: u64,
    pub tree_depth: u32,
}

/// Command sent when an mb-voucher is received from a buyer.
pub struct MbVoucherCommand {
    pub buyer_did: String,
    pub buyer_pubkey: String,
    pub order_id: String,
    pub channel_id: String,
    pub seq: u64,
    pub amount: u64,
    pub buyer_sig: String,
}

/// DIDComm mediator connection for the merchant MCP.
/// Simplified version — focuses on sending payment confirmations.
pub struct MerchantMediator {
    agent: Arc<Mutex<DIDCommAgent>>,
    did_doc: Value,
    our_did: String,
    ws_url: String,
    connected: Arc<Notify>,
    outgoing: Arc<tokio::sync::Mutex<mpsc::UnboundedSender<String>>>,
    /// DID of the paired merchant app (set during connection-request handshake).
    paired_phone: Arc<tokio::sync::Mutex<Option<String>>>,
    /// DID of an app that has sent connection-request but not yet confirmed (pending 3-step handshake).
    pending_phone: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Mediator HTTP URL of the paired app (used to forward messages via HTTP POST).
    phone_mediator_http_url: Arc<tokio::sync::Mutex<Option<String>>>,
    signing_private: [u8; 32],
    db: sled::Db,
    /// Channel for forwarding received MB vouchers to the MCP server for processing.
    mb_voucher_tx: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedSender<MbVoucherCommand>>>>,
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

impl std::fmt::Debug for MerchantMediator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MerchantMediator")
            .field("our_did", &self.our_did)
            .field("ws_url", &self.ws_url)
            .finish()
    }
}

impl MerchantMediator {
    pub fn new(ws_url: &str, db: &sled::Db) -> Result<Self> {
        let (identity, did) = match load_identity(db)? {
            Some(loaded) => {
                let did = loaded.did.clone();
                tracing::info!("Loaded existing merchant identity: {}", did);
                (loaded, did)
            }
            None => {
                let (id, did) = generate_ignite_did();
                tracing::info!("Generated new merchant identity: {}", did);
                save_identity(db, &id, &did)?;
                (id, did)
            }
        };

        let did_doc = build_did_document(&did, &identity);
        let signing_private = identity.signing_private
            .ok_or_else(|| anyhow::anyhow!("no signing key in identity"))?;
        let (agent, _) = didcomm::create_agent(identity);
        let (outgoing_tx, _) = mpsc::unbounded_channel();

        Ok(MerchantMediator {
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
            mb_voucher_tx: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    pub fn our_did(&self) -> &str {
        &self.our_did
    }

    pub fn did_doc(&self) -> &Value {
        &self.did_doc
    }

    /// Get the paired merchant app DID (if any app has completed pairing).
    pub async fn paired_phone_did(&self) -> Option<String> {
        self.paired_phone.lock().await.clone()
    }

    /// Generate an Out-of-Band invitation URL for P2P pairing.
    pub fn generate_invitation(&self) -> String {
        // Minimal invitation with routing info for message delivery.
        let invitation = serde_json::json!({
            "type": "https://didcomm.org/out-of-band/2.0/invitation",
            "from": self.our_did,
            "body": {
                "services": [{
                    "service_endpoint": self.ws_url,
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
            "Ignite Pay Merchant MCP",
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

    /// Connect to mediator and start background loop.
    pub async fn connect(
        &self,
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
        let mb_voucher_tx = self.mb_voucher_tx.clone();

        let (outgoing_tx, _outgoing_rx) = mpsc::unbounded_channel();
        {
            let mut guard = self.outgoing.lock().await;
            *guard = outgoing_tx;
        }

        let create_channel_tx = create_channel_tx.map(Arc::new);

        tokio::spawn(async move {
            loop {
                // Create a fresh channel pair for each connection attempt
                let (_tx, rx) = mpsc::unbounded_channel();
                match connect_and_run(
                    &ws_url,
                    &agent,
                    &our_did,
                    &did_doc,
                    connected.clone(),
                    rx,
                    &create_channel_tx,
                    &paired_phone,
                    &pending_phone,
                    &phone_mediator_http_url,
                    &signing_private,
                    &db,
                    &mb_voucher_tx,
                )
                .await
                {
                    Ok(()) => tracing::warn!("Mediator disconnected, reconnecting..."),
                    Err(e) => tracing::error!("WS error: {}, reconnecting in 3s...", e),
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });

        Ok(())
    }

    /// Send a channel payment confirmation to a user.
    /// Encrypts to JWE, wraps in forward message, sends directly to the app's mediator.
    pub async fn send_payment_confirmation(
        &self,
        user_did: &str,
        order_id: &str,
        channel_id: &str,
        leaf_index: u32,
        sequence: u64,
    ) -> Result<String> {
        let msg = didcomm::build_channel_payment_confirm(
            &self.our_did,
            user_did,
            order_id,
            channel_id,
            leaf_index,
            sequence,
        );

        let agent = self.agent.lock().await;
        let jwe = didcomm::pack_encrypted(&agent, &msg, &self.our_did, user_did)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        drop(agent);

        self.send_to_phone_mediator(user_did, &jwe).await?;

        tracing::info!("Payment confirmation sent to {} for order {}", user_did, order_id);
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

    /// Send a JWE to the app's mediator via HTTP POST.
    /// Wraps the JWE in a forward message so the mediator routes it to the app.
    /// Uses the mediator's public POST / endpoint (no auth required).
    async fn send_to_phone_mediator(&self, phone_did: &str, jwe: &str) -> Result<()> {
        let phone_http_url = self.phone_mediator_http_url.lock().await.clone();

        // If same mediator, send through our own outgoing channel
        let same_mediator = match &phone_http_url {
            Some(url) => {
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
            .ok_or_else(|| anyhow::anyhow!("App mediator HTTP URL not known (not paired?)"))?;

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
            "Sending forward-wrapped JWE to app {} via their mediator HTTP {}",
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
                "App mediator rejected message: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }

    /// Register a peer for encrypted communication.
    pub async fn add_peer_from_doc(&self, did: &str, doc: &Value) {
        if let Some(resolved) = parse_did_document(did, doc) {
            let mut agent = self.agent.lock().await;
            agent.add_peer(resolved);
            tracing::info!("Registered peer from DID document: {}", did);
        }
    }

    /// Set the MB voucher channel for forwarding received vouchers.
    pub async fn set_mb_voucher_channel(&self, tx: mpsc::UnboundedSender<MbVoucherCommand>) {
        let mut guard = self.mb_voucher_tx.lock().await;
        *guard = Some(tx);
    }
}

/// Forward a message to a phone's mediator via HTTP POST.
async fn merchant_http_forward(phone_did: &str, inner_msg: &str, phone_http_url: &str) -> Result<()> {
    let forward_msg = serde_json::json!({
        "type": "https://didcomm.org/routing/2.0/forward",
        "id": format!("fwd-{}", uuid::Uuid::new_v4()),
        "body": { "next": phone_did },
        "attachments": [{
            "data": { "json": serde_json::from_str::<serde_json::Value>(inner_msg).unwrap_or_else(|_| serde_json::Value::String(inner_msg.to_string())) }
        }]
    });
    let forward_str = serde_json::to_string(&forward_msg)?;
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
        return Err(anyhow::anyhow!("App mediator rejected: {} - {}", status, body));
    }
    Ok(())
}

/// Build and send a connection-confirm-response to the merchant app.
async fn merchant_send_conn_response(
    phone_did: &str,
    phone_http_url: &str,
    our_did: &str,
    did_doc: &Value,
    our_ws_url: &str,
    signing_private: &[u8; 32],
    accepted: bool,
) {
    // Derive our HTTP URL from our WS URL
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
    match merchant_http_forward(phone_did, &msg_str, phone_http_url).await {
        Ok(()) => tracing::info!("Sent connection-response to {} (accepted: {})", phone_did, accepted),
        Err(e) => tracing::error!("Failed to send connection-response: {}", e),
    }
}

async fn connect_and_run(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    _connected: Arc<Notify>,
    mut outgoing_rx: mpsc::UnboundedReceiver<String>,
    create_channel_tx: &Option<Arc<mpsc::UnboundedSender<CreateChannelCommand>>>,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    signing_private: &[u8; 32],
    db: &sled::Db,
    mb_voucher_tx: &Arc<tokio::sync::Mutex<Option<mpsc::UnboundedSender<MbVoucherCommand>>>>,
) -> Result<()> {
    let (mut ws, _) = connect_async(ws_url).await?;
    tracing::info!("Merchant connected to mediator: {}", ws_url);

    // Simplified handshake: mediate-request → keylist-update → peer-introduction
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;

    // Read grant (discard for now)
    let _ = read_msg(&mut ws).await?;

    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;

    let _ = read_msg(&mut ws).await?;

    let intro = didcomm::build_peer_introduction(our_did, did_doc);
    send_msg(&mut ws, serde_json::to_string(&intro)?).await?;

    tracing::info!("Merchant mediator handshake complete");

    // Bidirectional loop
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_incoming_message(&text, agent, create_channel_tx, paired_phone, pending_phone, phone_mediator_http_url, signing_private, db, our_did, did_doc, ws_url, mb_voucher_tx).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }
            jwe = outgoing_rx.recv() => {
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

async fn handle_incoming_message(
    text: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    create_channel_tx: &Option<Arc<mpsc::UnboundedSender<CreateChannelCommand>>>,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    signing_private: &[u8; 32],
    db: &sled::Db,
    our_did: &str,
    did_doc: &Value,
    mcp_ws_url: &str,
    mb_voucher_tx: &Arc<tokio::sync::Mutex<Option<mpsc::UnboundedSender<MbVoucherCommand>>>>,
) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg, agent, create_channel_tx, paired_phone, pending_phone, phone_mediator_http_url, signing_private, db, our_did, did_doc, mcp_ws_url, mb_voucher_tx).await;
                return;
            }
            Err(e) => {
                tracing::debug!("JWE unpack failed: {}, trying plaintext", e);
                drop(agent_guard);
            }
        }
    }

    // Plaintext fallback
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to parse message: {}", e);
            return;
        }
    };

    // Check for connection-request in plaintext (pairing from merchant app)
    if v
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t.contains("connection-request"))
        .unwrap_or(false)
    {
        let phone_did = v["from"].as_str().unwrap_or("");
        let app_http_url = v["body"]
            .get("mediator_http_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        tracing::info!(
            "Received plaintext connection-request from merchant app: {} (mediator: {:?})",
            phone_did,
            app_http_url
        );

        // Check if already paired — only first-time pairing is allowed
        {
            let guard = paired_phone.lock().await;
            if guard.is_some() {
                tracing::warn!("Rejecting pairing from {}: already paired", phone_did);
                drop(guard);
                if let Some(ref http_url) = app_http_url {
                    merchant_send_conn_response(phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, false).await;
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

        if let Some(phone_doc) = v["body"].get("did_document") {
            if let Some(resolved) = parse_did_document(phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered merchant app peer from DID document: {}", phone_did);
            }
        }

        // Store as pending (not yet fully paired — needs connection-confirm)
        {
            let mut guard = pending_phone.lock().await;
            *guard = Some(phone_did.to_string());
        }

        // Store the app's mediator HTTP URL
        if let Some(ref http_url) = app_http_url {
            let mut guard = phone_mediator_http_url.lock().await;
            *guard = Some(http_url.clone());
            save_phone_mediator_http_url(db, http_url);
            tracing::info!("Saved app mediator HTTP URL: {}", http_url);
        }

        tracing::info!("Merchant app {} connection-request stored as pending", phone_did);

        // Send connection-response back to app with MCP's identity info
        if let Some(ref http_url) = app_http_url {
            merchant_send_conn_response(phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, true).await;
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
            "Received plaintext connection-confirm from app: {} (nonce: {}...)",
            phone_did,
            &phone_nonce[..phone_nonce.len().min(8)]
        );

        // Verify that this app has a pending pairing
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
            tracing::warn!("App {} signature verification FAILED, rejecting", phone_did);
            let mut guard = pending_phone.lock().await;
            *guard = None;
            return;
        }

        tracing::info!("App {} signature verified, completing pairing", phone_did);

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

        tracing::info!("App {} fully paired", phone_did);
        return;
    }

    tracing::debug!(
        "Received non-auth message: {}",
        text.chars().take(100).collect::<String>()
    );
}

/// Process an unpacked DIDComm Message (from JWE).
async fn process_inner_message(
    msg: &affinidi_messaging_didcomm::Message,
    agent: &Arc<Mutex<DIDCommAgent>>,
    create_channel_tx: &Option<Arc<mpsc::UnboundedSender<CreateChannelCommand>>>,
    paired_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    pending_phone: &Arc<tokio::sync::Mutex<Option<String>>>,
    phone_mediator_http_url: &Arc<tokio::sync::Mutex<Option<String>>>,
    signing_private: &[u8; 32],
    db: &sled::Db,
    our_did: &str,
    did_doc: &Value,
    mcp_ws_url: &str,
    mb_voucher_tx: &Arc<tokio::sync::Mutex<Option<mpsc::UnboundedSender<MbVoucherCommand>>>>,
) {
    // Check for connection-request type (pairing from merchant app)
    if msg.typ.contains("connection-request") {
        let phone_did = msg.from.clone().unwrap_or_default();
        let app_http_url = msg
            .body
            .get("mediator_http_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        tracing::info!(
            "Received connection-request from merchant app: {} (mediator: {:?})",
            phone_did,
            app_http_url
        );

        // Check if already paired — only first-time pairing is allowed
        {
            let guard = paired_phone.lock().await;
            if guard.is_some() {
                tracing::warn!("Rejecting pairing from {}: already paired", phone_did);
                drop(guard);
                if let Some(ref http_url) = app_http_url {
                    merchant_send_conn_response(&phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, false).await;
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

        if let Some(phone_doc) = msg.body.get("did_document") {
            if let Some(resolved) = parse_did_document(&phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered merchant app peer from DID document: {}", phone_did);
            }
        }

        // Store as pending (not yet fully paired — needs connection-confirm)
        {
            let mut guard = pending_phone.lock().await;
            *guard = Some(phone_did.clone());
        }

        // Store the app's mediator HTTP URL
        if let Some(ref http_url) = app_http_url {
            let mut guard = phone_mediator_http_url.lock().await;
            *guard = Some(http_url.clone());
            save_phone_mediator_http_url(db, http_url);
            tracing::info!("Saved app mediator HTTP URL: {}", http_url);
        }

        tracing::info!("Merchant app {} connection-request stored as pending", phone_did);

        // Send connection-response back to app with MCP's identity info
        if let Some(ref http_url) = app_http_url {
            merchant_send_conn_response(&phone_did, http_url, our_did, did_doc, mcp_ws_url, signing_private, true).await;
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
            "Received connection-confirm from app: {} (nonce: {}...)",
            phone_did,
            &phone_nonce[..phone_nonce.len().min(8)]
        );

        // Verify that this app has a pending pairing
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
            tracing::warn!("App {} signature verification FAILED, rejecting", phone_did);
            let mut guard = pending_phone.lock().await;
            *guard = None;
            return;
        }

        tracing::info!("App {} signature verified, completing pairing", phone_did);

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

        tracing::info!("App {} fully paired", phone_did);
        return;
    }

    // Handle MB voucher messages from buyers
    if msg.typ.contains("mb-voucher") {
        let buyer_did = msg.from.clone().unwrap_or_default();
        let buyer_pubkey = msg
            .body
            .get("buyer_pubkey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let order_id = msg
            .body
            .get("order_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let channel_id = msg
            .body
            .get("channel_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let seq = msg.body.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
        let amount = msg.body.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
        let buyer_sig = msg
            .body
            .get("buyer_sig")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        tracing::info!(
            "Received mb-voucher from buyer {} (order: {}, seq: {}, amount: {})",
            buyer_did,
            order_id,
            seq,
            amount
        );

        // Forward to MCP server for processing
        let tx_guard = mb_voucher_tx.lock().await;
        if let Some(tx) = tx_guard.as_ref() {
            let cmd = MbVoucherCommand {
                buyer_did,
                buyer_pubkey,
                order_id,
                channel_id,
                seq,
                amount,
                buyer_sig,
            };
            if let Err(e) = tx.send(cmd) {
                tracing::error!("Failed to forward MB voucher command: {}", e);
            }
        } else {
            tracing::warn!("No MB voucher handler registered, ignoring mb-voucher");
        }

        return;
    }

    if msg.typ.contains("create-channel-request") {
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
        tracing::debug!("Received message type={}, no handler", msg.typ);
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
