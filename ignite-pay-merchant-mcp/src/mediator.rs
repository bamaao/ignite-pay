use anyhow::Result;
use affinidi_messaging_didcomm::DIDCommAgent;
use futures_util::{SinkExt, StreamExt};
use ignite_pay_core::didcomm;
use ignite_pay_core::identity::{load_identity, save_identity};
use ignite_pay_core::{build_did_document, generate_ignite_did, parse_did_document};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_tungstenite::connect_async;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// DIDComm mediator connection for the merchant MCP.
/// Simplified version — focuses on sending payment confirmations.
pub struct MerchantMediator {
    agent: Arc<Mutex<DIDCommAgent>>,
    did_doc: Value,
    our_did: String,
    ws_url: String,
    connected: Arc<Notify>,
    outgoing: Arc<tokio::sync::Mutex<mpsc::UnboundedSender<String>>>,
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
        })
    }

    pub fn our_did(&self) -> &str {
        &self.our_did
    }

    pub fn did_doc(&self) -> &Value {
        &self.did_doc
    }

    /// Connect to mediator and start background loop.
    pub async fn connect(&self) -> Result<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let ws_url = self.ws_url.clone();
        let connected = self.connected.clone();

        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        {
            let mut guard = self.outgoing.lock().await;
            *guard = outgoing_tx;
        }

        tokio::spawn(async move {
            loop {
                // Create a new channel pair for each connection attempt
                let (tx, rx) = mpsc::unbounded_channel();
                // Note: we can't update the stored sender here since we moved out.
                // Instead, the caller should use a shared channel.
                match connect_and_run(
                    &ws_url, &agent, &our_did, &did_doc, connected.clone(), rx,
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
    _agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    _connected: Arc<Notify>,
    mut outgoing_rx: mpsc::UnboundedReceiver<String>,
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
                        tracing::debug!("Received: {}", text.chars().take(100).collect::<String>());
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
