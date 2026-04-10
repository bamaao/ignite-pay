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

use affinidi_messaging_didcomm::DIDCommAgent;
use affinidi_messaging_didcomm::identity::PrivateIdentity;
use ignite_pay_core::{generate_ignite_did, build_did_document, identity_to_resolved};
use ignite_pay_core::didcomm::{self, is_jwe};

// --- Global task coordinator (keyed by payment_id) ---
lazy_static! {
    static ref PENDING_TASKS: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[pyclass]
struct IgnitePayCore {
    agent: Arc<Mutex<DIDCommAgent>>,
    our_did: String,
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
}

#[pymethods]
impl IgnitePayCore {
    #[new]
    fn new() -> Self {
        let (priv_identity, did) = generate_ignite_did();
        let (agent, _) = didcomm::create_agent(priv_identity);

        IgnitePayCore {
            agent: Arc::new(Mutex::new(agent)),
            our_did: did,
            outgoing: Arc::new(Mutex::new(None)),
        }
    }

    /// Register mediator's resolved identity as a peer in the DIDComm agent.
    fn add_mediator_peer(&self, mediator_did: String) -> PyResult<()> {
        let mediator_identity = PrivateIdentity::generate(&mediator_did);
        let resolved = identity_to_resolved(&mediator_identity);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut agent = self.agent.lock().await;
            agent.add_peer(resolved);
        });

        Ok(())
    }

    /// Start background WebSocket listener
    fn start_listener(&self, _py: Python, ws_url: String) -> PyResult<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        let outgoing = self.outgoing.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                println!("DIDComm WebSocket listener starting: {} (DID: {})", ws_url, our_did);
                real_ws_client(&ws_url, &agent, &our_did, outgoing).await;
            });
        });
        Ok(())
    }

    /// Core payment interface: Pub/Sub pattern
    fn check_and_pay<'p>(&self, py: Python<'p>, payment_id: String, merchant_did: String, amount: u64) -> PyResult<&'p PyAny> {
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
}

/// Reconnecting WebSocket client loop.
async fn real_ws_client(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    outgoing: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
) {
    loop {
        match connect_and_run(ws_url, agent, our_did, &outgoing).await {
            Ok(()) => println!("Mediator disconnected, reconnecting..."),
            Err(e) => eprintln!("WS error: {}, reconnecting in 3s...", e),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Connect to mediator, perform plaintext handshake, then enter bidirectional loop.
async fn connect_and_run(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
    outgoing: &Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(ws_url).await?;
    println!("Connected to Mediator: {}", ws_url);

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
    {
        let temp = PrivateIdentity::generate(our_did);
        let did_doc = build_did_document(our_did, &temp);
        let intro = didcomm::build_peer_introduction(our_did, &did_doc);
        send_msg(&mut ws, serde_json::to_string(&intro)?).await?;
        println!("Sent peer-introduction (DID doc)");
    }

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
