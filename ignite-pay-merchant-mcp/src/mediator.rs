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
            db: db.clone(),
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
        let db = self.db.clone();

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
                    &db,
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

    /// Send a channel payment confirmation to a user via the mediator.
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

        let sender = self.outgoing.lock().await;
        sender
            .send(jwe.clone())
            .map_err(|_| anyhow::anyhow!("WebSocket channel closed"))?;

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

        let sender = self.outgoing.lock().await;
        sender
            .send(jwe.clone())
            .map_err(|_| anyhow::anyhow!("WebSocket channel closed"))?;

        tracing::info!(
            "Create channel response sent to {}: success={}, channel_id={}",
            app_did,
            success,
            channel_id
        );

        Ok(jwe)
    }

    /// Register a peer for encrypted communication.
    pub async fn add_peer_from_doc(&self, did: &str, doc: &Value) {
        if let Some(resolved) = parse_did_document(did, doc) {
            let mut agent = self.agent.lock().await;
            agent.add_peer(resolved);
            tracing::info!("Registered peer from DID document: {}", did);
        }
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
    db: &sled::Db,
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
                        handle_incoming_message(&text, agent, create_channel_tx, paired_phone, db).await;
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
    db: &sled::Db,
) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg, agent, create_channel_tx, paired_phone, db).await;
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

        tracing::info!(
            "Received plaintext connection-request from merchant app: {}",
            phone_did
        );

        if let Some(phone_doc) = v["body"].get("did_document") {
            if let Some(resolved) = parse_did_document(phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered merchant app peer from DID document: {}", phone_did);
            }
        }

        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.to_string());
        }
        save_paired_phone(db, phone_did);

        tracing::info!("Merchant app {} paired successfully via plaintext", phone_did);
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
    db: &sled::Db,
) {
    // Check for connection-request type (pairing from merchant app)
    if msg.typ.contains("connection-request") {
        let phone_did = msg.from.clone().unwrap_or_default();

        tracing::info!(
            "Received connection-request from merchant app: {}",
            phone_did
        );

        if let Some(phone_doc) = msg.body.get("did_document") {
            if let Some(resolved) = parse_did_document(&phone_did, phone_doc) {
                let mut agent_guard = agent.lock().await;
                agent_guard.add_peer(resolved);
                tracing::info!("Registered merchant app peer from DID document: {}", phone_did);
            }
        }

        {
            let mut guard = paired_phone.lock().await;
            *guard = Some(phone_did.clone());
        }
        save_paired_phone(db, &phone_did);

        tracing::info!("Merchant app {} paired successfully", phone_did);
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
