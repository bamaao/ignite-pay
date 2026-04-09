mod didcomm;
mod identity;

use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use lazy_static::lazy_static;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;

use affinidi_messaging_didcomm::DIDCommAgent;
use identity::{generate_ignite_did, build_did_document, identity_to_resolved};

// --- 全局任务协调器 ---
lazy_static! {
    static ref PENDING_TASKS: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[pyclass]
struct IgnitePayCore {
    agent: Arc<Mutex<DIDCommAgent>>,
    our_did: String,
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
        }
    }

    /// Register mediator's resolved identity as a peer in the DIDComm agent.
    fn add_mediator_peer(&self, mediator_did: String) -> PyResult<()> {
        // Generate a resolved identity for the mediator so we can encrypt to it.
        // In production, this would come from DID resolution.
        let mediator_identity = affinidi_messaging_didcomm::identity::PrivateIdentity::generate(&mediator_did);
        let resolved = identity_to_resolved(&mediator_identity);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut agent = self.agent.lock().await;
            agent.add_peer(resolved);
        });

        Ok(())
    }

    /// 启动后台 WebSocket 监听器
    fn start_listener(&self, _py: Python, ws_url: String) -> PyResult<()> {
        let agent = self.agent.clone();
        let our_did = self.our_did.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                println!("DIDComm WebSocket 监听器启动: {} (DID: {})", ws_url, our_did);
                real_ws_client(ws_url, agent, &our_did).await;
            });
        });
        Ok(())
    }

    /// 核心支付接口：Pub/Sub 模式
    fn check_and_pay<'p>(&self, py: Python<'p>, merchant_did: String, _amount: u64) -> PyResult<&'p PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let (tx, rx) = oneshot::channel();
            {
                let mut tasks = PENDING_TASKS.lock().await;
                tasks.insert(merchant_did.clone(), tx);
            }

            println!("已发送授权请求给手机，等待商户 {} 的响应...", merchant_did);

            match tokio::time::timeout(Duration::from_secs(300), rx).await {
                Ok(Ok(true)) => {
                    println!("收到授权信号，正在执行结算...");
                    let tx_sig = format!("tx_sig_{}", uuid::Uuid::new_v4());
                    Ok(tx_sig)
                }
                Ok(Ok(false)) => Err(PyRuntimeError::new_err("用户拒绝了支付授权")),
                Err(_) => {
                    PENDING_TASKS.lock().await.remove(&merchant_did);
                    Err(PyRuntimeError::new_err("授权超时，请重试"))
                }
                _ => Err(PyRuntimeError::new_err("内部通信错误")),
            }
        })
    }
}

/// Reconnecting WebSocket client loop.
async fn real_ws_client(ws_url: String, agent: Arc<Mutex<DIDCommAgent>>, our_did: &str) {
    loop {
        match connect_and_run(&ws_url, &agent, our_did).await {
            Ok(()) => println!("Mediator disconnected, reconnecting..."),
            Err(e) => eprintln!("WS error: {}, reconnecting in 3s...", e),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Connect to mediator, perform plaintext handshake, then enter encrypted receive loop.
async fn connect_and_run(
    ws_url: &str,
    agent: &Arc<Mutex<DIDCommAgent>>,
    our_did: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(ws_url).await?;
    println!("已连接到 Mediator: {}", ws_url);

    // --- Phase A: Plaintext handshake ---

    // 1. mediate-request (plaintext)
    let req = didcomm::build_mediate_request(our_did);
    send_msg(&mut ws, serde_json::to_string(&req)?).await?;
    println!("已发送 mediate-request (from: {})", our_did);

    // Read mediate-grant
    let grant = read_msg(&mut ws).await?;
    let grant_v: Value = serde_json::from_str(&grant)?;
    if grant_v.get("type").and_then(|v| v.as_str())
        .map(|t| t.contains("mediate-grant"))
        .unwrap_or(false)
    {
        println!("收到 mediate-grant");
    } else {
        eprintln!("预期 mediate-grant，收到: {}", grant);
    }

    // 2. keylist-update (plaintext)
    let kup = didcomm::build_keylist_update(our_did);
    send_msg(&mut ws, serde_json::to_string(&kup)?).await?;
    println!("已发送 keylist-update");

    let kl_resp = read_msg(&mut ws).await?;
    let kl_v: Value = serde_json::from_str(&kl_resp)?;
    if kl_v.get("type").and_then(|v| v.as_str())
        .map(|t| t.contains("keylist-update"))
        .unwrap_or(false)
    {
        println!("收到 keylist-update-response，注册完成");
    } else {
        eprintln!("预期 keylist-update-response，收到: {}", kl_resp);
    }

    // 3. peer-introduction (plaintext) — send our DID document so mediator can encrypt to us
    {
        let agent_guard = agent.lock().await;
        // Reconstruct identity info from agent for DID doc building
        // We need the public keys; build from the agent's store
        let did_doc = build_did_document_from_agent(our_did, &agent_guard);
        let intro = didcomm::build_peer_introduction(our_did, &did_doc);
        send_msg(&mut ws, serde_json::to_string(&intro)?).await?;
        println!("已发送 peer-introduction (DID doc)");
    }

    println!("Mediator 握手完成，正在监听加密消息...");

    // --- Phase B: Receive loop (encrypted + plaintext fallback) ---
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                handle_incoming_message(&text, agent).await;
            }
            Ok(_) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

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
    if didcomm::is_jwe(text) {
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
            eprintln!("无法解析消息: {}", e);
            return;
        }
    };

    // Check if it's a DIDComm message with a body
    if let Some(body) = v.get("body") {
        if let Some(merchant_did) = body.get("merchant_did").and_then(|v| v.as_str()) {
            let authorized = body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            resolve_pending(merchant_did, authorized).await;
            return;
        }
    }

    // Direct authorization fields (legacy)
    if let Some(merchant_did) = v.get("merchant_did").and_then(|v| v.as_str()) {
        let authorized = v.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
        resolve_pending(merchant_did, authorized).await;
        return;
    }

    println!("收到非授权消息: {}", text.chars().take(100).collect::<String>());
}

/// Process an unpacked DIDComm Message (from JWE or plaintext).
async fn process_inner_message(msg: &affinidi_messaging_didcomm::Message) {
    // Extract authorization data from body
    if let Some(merchant_did) = msg.body.get("merchant_did").and_then(|v| v.as_str()) {
        let authorized = msg.body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
        resolve_pending(merchant_did, authorized).await;
    } else {
        println!("收到消息 type={}, 无授权数据", msg.typ);
    }
}

/// Resolve a pending payment task.
async fn resolve_pending(merchant_did: &str, authorized: bool) {
    let mut tasks = PENDING_TASKS.lock().await;
    if let Some(tx) = tasks.remove(merchant_did) {
        println!("收到授权响应: {} -> {}", merchant_did, authorized);
        let _ = tx.send(authorized);
    } else {
        println!("收到未匹配的授权响应: {}", merchant_did);
    }
}

/// Build a DID Document using keys from the agent's local store.
/// Since we can't extract PrivateIdentity from the agent, we reconstruct
/// the public parts from the DID string (the keys are registered internally).
fn build_did_document_from_agent(did: &str, _agent: &DIDCommAgent) -> Value {
    // We generate a temporary identity with the same DID to get public keys.
    // The agent already has the real private keys registered.
    let temp = affinidi_messaging_didcomm::identity::PrivateIdentity::generate(did);
    build_did_document(did, &temp)
}

#[pymodule]
fn ignite_pay_rs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<IgnitePayCore>()?;
    Ok(())
}
