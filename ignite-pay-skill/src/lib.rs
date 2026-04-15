use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use lazy_static::lazy_static;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;
use chrono::Utc;

use affinidi_messaging_didcomm::DIDCommAgent;
use ignite_pay_core::{generate_ignite_did, build_did_document, parse_did_document};
use ignite_pay_core::didcomm::{self, is_jwe};
use ignite_pay_core::identity::{load_identity, save_identity};
use ignite_pay_core::list_store::ListStore;
use ignite_pay_core::types::{MerchantListEntry, WhitelistResult, RiskControlDecision};

// --- Global task coordinator (keyed by payment_id) ---
lazy_static! {
    static ref PENDING_TASKS: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[pyclass]
struct IgnitePayCore {
    agent: Arc<Mutex<DIDCommAgent>>,
    our_did: String,
    did_doc: Value,
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    list_store: Arc<Mutex<Option<ListStore>>>,
    identity_db: Option<sled::Db>,
}

#[pymethods]
impl IgnitePayCore {
    #[new]
    fn new() -> Self {
        let (priv_identity, did) = generate_ignite_did();
        let did_doc = build_did_document(&did, &priv_identity);
        let (agent, _) = didcomm::create_agent(priv_identity);

        IgnitePayCore {
            agent: Arc::new(Mutex::new(agent)),
            our_did: did,
            did_doc,
            outgoing: Arc::new(Mutex::new(None)),
            list_store: Arc::new(Mutex::new(None)),
            identity_db: None,
        }
    }

    /// Initialize persistent identity from a sled database.
    /// If a previous identity exists, loads it with the same private keys.
    /// Otherwise generates a new one and saves it.
    #[pyo3(signature = (db_path))]
    fn init_identity(&mut self, db_path: String) -> PyResult<()> {
        let db = sled::open(&db_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to open database: {}", e)))?;

        let (identity, did) = match load_identity(&db)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to load identity: {}", e)))?
        {
            Some(loaded) => {
                let did = loaded.did.clone();
                (loaded, did)
            }
            None => {
                let (id, did) = generate_ignite_did();
                save_identity(&db, &id, &did)
                    .map_err(|e| PyRuntimeError::new_err(format!("Failed to save identity: {}", e)))?;
                (id, did)
            }
        };

        let did_doc = build_did_document(&did, &identity);
        let (agent, _) = didcomm::create_agent(identity);

        self.agent = Arc::new(Mutex::new(agent));
        self.our_did = did;
        self.did_doc = did_doc;
        self.identity_db = Some(db);

        Ok(())
    }

    /// Initialize sled-backed ListStore for whitelist/blacklist persistence.
    #[pyo3(signature = (db_path))]
    fn init_list_store(&self, db_path: String) -> PyResult<()> {
        let db = sled::open(&db_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to open database: {}", e)))?;
        let store = ListStore::new(db);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut guard = self.list_store.lock().await;
            *guard = Some(store);
        });
        Ok(())
    }

    /// Start background WebSocket listener with WS challenge-response authentication.
    fn start_listener(&self, _py: Python, ws_url: String) -> PyResult<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let did_doc = self.did_doc.clone();
        let outgoing = self.outgoing.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                println!("DIDComm WebSocket listener starting: {} (DID: {})", ws_url, our_did);
                real_ws_client(&ws_url, &agent, &our_did, &did_doc, outgoing).await;
            });
        });
        Ok(())
    }

    /// Query allowance for a merchant. Returns JSON string with allowance info.
    #[pyo3(signature = (merchant_did, amount=None))]
    fn check_allowance(&self, merchant_did: String, amount: Option<u64>) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Ok(serde_json::json!({
                    "error": "ListStore not initialized. Call init_list_store() first."
                }).to_string()),
            };

            let is_blacklisted = store.is_blacklisted(&merchant_did)
                .map_err(|e| PyRuntimeError::new_err(format!("Blacklist check failed: {}", e)))?;

            if is_blacklisted {
                return Ok(serde_json::json!({
                    "is_blacklisted": true,
                    "is_whitelisted": false,
                    "max_amount": null,
                    "label": null,
                    "expires_at": null,
                }).to_string());
            }

            let check_amount = amount.unwrap_or(0);
            let result: WhitelistResult = store.check_whitelist(&merchant_did, check_amount)
                .map_err(|e| PyRuntimeError::new_err(format!("Whitelist check failed: {}", e)))?;

            Ok(serde_json::json!({
                "is_blacklisted": false,
                "is_whitelisted": result.is_whitelisted,
                "max_amount": result.max_amount,
                "label": result.label,
                "expires_at": result.expires_at.map(|dt| dt.to_rfc3339()),
            }).to_string())
        })
    }

    /// Risk check for a merchant and amount. Returns JSON with the decision.
    #[pyo3(signature = (merchant_did, amount))]
    fn risk_check(&self, merchant_did: String, amount: u64) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Ok(serde_json::json!({
                    "decision": "needs_auth",
                    "reason": "ListStore not initialized"
                }).to_string()),
            };

            let decision: RiskControlDecision = store.risk_check(&merchant_did, amount)
                .map_err(|e| PyRuntimeError::new_err(format!("Risk check failed: {}", e)))?;

            match decision {
                RiskControlDecision::Blocked => Ok(serde_json::json!({
                    "decision": "blocked",
                    "reason": "Merchant is blacklisted"
                }).to_string()),
                RiskControlDecision::AutoApproved { max_amount, label } => Ok(serde_json::json!({
                    "decision": "auto_approved",
                    "max_amount": max_amount,
                    "label": label,
                }).to_string()),
                RiskControlDecision::NeedsAuth => Ok(serde_json::json!({
                    "decision": "needs_auth"
                }).to_string()),
            }
        })
    }

    /// Core payment interface. payment_id is auto-generated.
    /// Python signature: check_and_pay(merchant_did, amount)
    fn check_and_pay<'p>(&self, py: Python<'p>, merchant_did: String, amount: u64) -> PyResult<&'p PyAny> {
        let payment_id = format!("pay_{}", uuid::Uuid::new_v4());
        let outgoing = self.outgoing.clone();
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let (tx, rx) = oneshot::channel();
            {
                let mut tasks = PENDING_TASKS.lock().await;
                tasks.insert(payment_id.clone(), tx);
            }

            // Build and send auth request via outgoing channel
            let msg = didcomm::build_authorization_request(
                &our_did,
                &merchant_did,
                &payment_id,
                &merchant_did,
                amount,
                "Payment authorization request",
            );

            let jwe = {
                let agent_guard = agent.lock().await;
                match didcomm::pack_encrypted(&agent_guard, &msg, &our_did, &merchant_did) {
                    Ok(jwe) => jwe,
                    Err(e) => {
                        drop(agent_guard);
                        PENDING_TASKS.lock().await.remove(&payment_id);
                        return Err(PyRuntimeError::new_err(format!("Encryption failed: {}", e)));
                    }
                }
            };

            // Send through outgoing WS channel
            {
                let outgoing_guard = outgoing.lock().await;
                if let Some(sender) = outgoing_guard.as_ref() {
                    if sender.send(jwe).is_err() {
                        PENDING_TASKS.lock().await.remove(&payment_id);
                        return Err(PyRuntimeError::new_err("WebSocket channel closed"));
                    }
                }
            }

            println!("Auth request sent for payment {}, waiting for response...", payment_id);

            match tokio::time::timeout(Duration::from_secs(300), rx).await {
                Ok(Ok(true)) => {
                    let tx_sig = format!("tx_sig_{}", uuid::Uuid::new_v4());
                    Ok(tx_sig)
                }
                Ok(Ok(false)) => Err(PyRuntimeError::new_err("User rejected the payment authorization")),
                Err(_) => {
                    PENDING_TASKS.lock().await.remove(&payment_id);
                    Err(PyRuntimeError::new_err("Authorization timed out, please retry"))
                }
                _ => Err(PyRuntimeError::new_err("Internal communication error")),
            }
        })
    }

    /// Add a merchant to the whitelist.
    #[pyo3(signature = (did, name=None, max_amount=None, label=None))]
    fn add_to_whitelist(&self, did: String, name: Option<String>, max_amount: Option<u64>, label: Option<String>) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Err(PyRuntimeError::new_err("ListStore not initialized. Call init_list_store() first.")),
            };
            let entry = MerchantListEntry {
                did: did.clone(),
                name,
                max_amount,
                added_at: Utc::now(),
                label,
                expires: None,
            };
            store.add_to_whitelist(entry)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to add to whitelist: {}", e)))?;
            Ok(())
        })
    }

    /// Remove a merchant from the whitelist.
    fn remove_from_whitelist(&self, did: String) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Err(PyRuntimeError::new_err("ListStore not initialized. Call init_list_store() first.")),
            };
            store.remove_from_whitelist(&did)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to remove from whitelist: {}", e)))?;
            Ok(())
        })
    }

    /// Add a merchant to the blacklist.
    #[pyo3(signature = (did, name=None))]
    fn add_to_blacklist(&self, did: String, name: Option<String>) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Err(PyRuntimeError::new_err("ListStore not initialized. Call init_list_store() first.")),
            };
            let entry = MerchantListEntry {
                did: did.clone(),
                name,
                max_amount: None,
                added_at: Utc::now(),
                label: None,
                expires: None,
            };
            store.add_to_blacklist(entry)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to add to blacklist: {}", e)))?;
            Ok(())
        })
    }

    /// Remove a merchant from the blacklist.
    fn remove_from_blacklist(&self, did: String) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let guard = self.list_store.lock().await;
            let store = match guard.as_ref() {
                Some(s) => s,
                None => return Err(PyRuntimeError::new_err("ListStore not initialized. Call init_list_store() first.")),
            };
            store.remove_from_blacklist(&did)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to remove from blacklist: {}", e)))?;
            Ok(())
        })
    }

    /// Get our DID.
    #[getter]
    fn our_did(&self) -> &str {
        &self.our_did
    }
}

/// Reconnecting WebSocket client loop.
async fn real_ws_client(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
) {
    loop {
        match connect_and_run(ws_url, agent, our_did, did_doc, &outgoing).await {
            Ok(()) => println!("Mediator disconnected, reconnecting..."),
            Err(e) => eprintln!("WS error: {}, reconnecting in 3s...", e),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Connect to mediator, perform WS challenge-response auth then plaintext handshake,
/// then enter bidirectional loop.
async fn connect_and_run(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    did_doc: &Value,
    outgoing: &Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(ws_url).await?;
    println!("Connected to Mediator: {}", ws_url);

    // --- Phase 0: WS Challenge-Response Authentication ---

    // 0a. Receive challenge
    let challenge_text = read_msg(&mut ws).await?;
    let challenge: Value = serde_json::from_str(&challenge_text)?;

    let challenge_type = challenge.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if !challenge_type.contains("ws-challenge") {
        return Err(format!("Expected ws-challenge, got type: {}", challenge_type).into());
    }

    let nonce = challenge["body"]["nonce"].as_str()
        .ok_or("Challenge missing body.nonce")?;
    let mediator_did = challenge["from"].as_str()
        .ok_or("Challenge missing from field")?;
    let mediator_doc = challenge["body"].get("did_document")
        .ok_or("Challenge missing body.did_document")?;

    println!("Received WS challenge from mediator: {}", mediator_did);

    // 0b. Register mediator as peer from their DID document
    {
        let mut agent_guard = agent.lock().await;
        if let Some(resolved) = parse_did_document(mediator_did, mediator_doc) {
            agent_guard.add_peer(resolved);
            println!("Registered mediator peer from DID document");
        } else {
            return Err("Failed to parse mediator DID document".into());
        }
    }

    // 0c. Build and send encrypted challenge response
    {
        let agent_guard = agent.lock().await;
        let response_msg = didcomm::build_ws_challenge_response(our_did, mediator_did, nonce, did_doc);
        let jwe = didcomm::pack_encrypted(&agent_guard, &response_msg, our_did, mediator_did)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
        send_msg(&mut ws, jwe).await?;
        println!("Sent WS challenge response");
    }

    // 0d. Wait for auth result
    let auth_result = read_msg(&mut ws).await?;
    let auth_v: Value = serde_json::from_str(&auth_result)?;
    let auth_type = auth_v.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if auth_type.contains("ws-auth-ok") {
        println!("WS authentication successful");
    } else if auth_type.contains("ws-auth-failed") {
        let reason = auth_v["body"]["reason"].as_str().unwrap_or("unknown");
        return Err(format!("WS authentication failed: {}", reason).into());
    } else {
        return Err(format!("Unexpected auth response type: {}", auth_type).into());
    }

    // --- Phase A: Plaintext handshake ---

    // 1. mediate-request
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;
    println!("Sent mediate-request (from: {})", our_did);

    let grant = read_msg(&mut ws).await?;
    let grant_v: Value = serde_json::from_str(&grant)?;
    if grant_v.get("type").and_then(|v| v.as_str())
        .map(|t| t.contains("mediate-grant"))
        .unwrap_or(false)
    {
        println!("Received mediate-grant");
    } else {
        eprintln!("Expected mediate-grant, got: {}", grant);
    }

    // 2. keylist-update
    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;
    println!("Sent keylist-update");

    let kl_resp = read_msg(&mut ws).await?;
    let kl_v: Value = serde_json::from_str(&kl_resp)?;
    if kl_v.get("type").and_then(|v| v.as_str())
        .map(|t| t.contains("keylist-update"))
        .unwrap_or(false)
    {
        println!("Received keylist-update-response, registration complete");
    } else {
        eprintln!("Expected keylist-update-response, got: {}", kl_resp);
    }

    // 3. peer-introduction — send our DID document
    let intro = didcomm::build_peer_introduction(our_did, did_doc);
    send_msg(&mut ws, serde_json::to_string(&intro)?).await?;
    println!("Sent peer-introduction (DID doc)");

    println!("Mediator handshake complete, entering bidirectional loop...");

    // Set up outgoing channel
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    {
        let mut guard = outgoing.lock().await;
        *guard = Some(out_tx);
    }

    // --- Phase B: Bidirectional loop using tokio::select! ---
    loop {
        tokio::select! {
            // Handle incoming messages
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_incoming_message(&text, agent).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }
            // Handle outgoing messages
            jwe = out_rx.recv() => {
                match jwe {
                    Some(msg) => {
                        if let Err(e) = send_msg(&mut ws, msg).await {
                            eprintln!("Failed to send outgoing message: {}", e);
                            return Err(e);
                        }
                    }
                    None => return Ok(()),
                }
            }
        }
    }
}

async fn send_msg(
    ws: &mut WsStream,
    msg: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.into())).await?;
    Ok(())
}

async fn read_msg(
    ws: &mut WsStream,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => return Ok(text.to_string()),
            Ok(_) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err("Connection closed".into())
}

/// Handle an incoming message: try JWE unpack first, then plaintext fallback.
async fn handle_incoming_message(text: &str, agent: &Arc<Mutex<DIDCommAgent>>) {
    // Try encrypted unpack first
    if is_jwe(text) {
        let agent_guard = agent.lock().await;
        match didcomm::unpack_message(&agent_guard, text, None) {
            Ok(msg) => {
                drop(agent_guard);
                process_inner_message(&msg).await;
                return;
            }
            Err(e) => {
                eprintln!("JWE unpack failed: {}, trying plaintext", e);
                drop(agent_guard);
            }
        }
    }

    // Plaintext fallback
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Cannot parse message: {}", e);
            return;
        }
    };

    // Check for auth response — use payment_id as key
    if let Some(body) = v.get("body") {
        if let Some(payment_id) = body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            resolve_pending(payment_id, authorized).await;
            return;
        }
    }

    // Direct authorization fields (legacy)
    if let Some(payment_id) = v.get("payment_id").and_then(|v| v.as_str()) {
        let authorized = v.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
        resolve_pending(payment_id, authorized).await;
        return;
    }

    println!("Received non-auth message: {}", text.chars().take(100).collect::<String>());
}

/// Process an unpacked DIDComm Message (from JWE or plaintext).
async fn process_inner_message(msg: &affinidi_messaging_didcomm::Message) {
    // Check for payment-auth-response type
    if msg.typ.contains("payment-auth-response") {
        if let Some(payment_id) = msg.body.get("payment_id").and_then(|v| v.as_str()) {
            let authorized = msg.body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            resolve_pending(payment_id, authorized).await;
        }
    } else {
        println!("Received message type={}, no auth data", msg.typ);
    }
}

/// Resolve a pending payment task (keyed by payment_id).
async fn resolve_pending(payment_id: &str, authorized: bool) {
    let mut tasks = PENDING_TASKS.lock().await;
    if let Some(tx) = tasks.remove(payment_id) {
        println!("Received auth response: {} -> {}", payment_id, authorized);
        let _ = tx.send(authorized);
    } else {
        println!("Received unmatched auth response: {}", payment_id);
    }
}

#[pymodule]
fn ignite_pay_rs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<IgnitePayCore>()?;
    Ok(())
}
