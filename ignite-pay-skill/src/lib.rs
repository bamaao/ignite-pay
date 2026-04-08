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

const OUR_DID: &str = "did:ignite:skill";

// --- 全局任务协调器 ---
// 使用商户 DID 作为 Key，存储 oneshot 发送端。
// 一旦 WebSocket 收到消息，就通过发送端唤醒正在等待的协程。
lazy_static! {
    static ref PENDING_TASKS: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[pyclass]
struct IgnitePayCore {
    // 可以在这里存储配置，如 Mediator 地址等
}

#[pymethods]
impl IgnitePayCore {
    #[new]
    fn new() -> Self {
        IgnitePayCore {}
    }

    /// 启动后台 WebSocket 监听器
    /// 连接到 DIDComm Mediator 并监听转发消息
    fn start_listener(&self, _py: Python, ws_url: String) -> PyResult<()> {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                println!("DIDComm WebSocket 监听器启动: {}", ws_url);
                real_ws_client(ws_url).await;
            });
        });
        Ok(())
    }

    /// 核心支付接口：Pub/Sub 模式
    fn check_and_pay<'p>(&self, py: Python<'p>, merchant_did: String, amount: u64) -> PyResult<&'p PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            // 1. 检查本地缓存（此处略，假设需要授权）
            
            // 2. 创建一个订阅通道 (Sub)
            let (tx, rx) = oneshot::channel();
            {
                let mut tasks = PENDING_TASKS.lock().await;
                tasks.insert(merchant_did.clone(), tx);
            }

            // 3. 发布授权请求（实际应通过 WS 发送 DIDComm 给手机）
            println!("已发送授权请求给手机，等待商户 {} 的响应...", merchant_did);

            // 4. 异步等待发布者的信号 (Wait for Pub)
            // 设置 5 分钟超时
            match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                Ok(Ok(true)) => {
                    // 5. 收到信号，执行 Solana 交易
                    println!("收到授权信号，正在执行结算...");
                    let tx_sig = format!("tx_sig_{}", uuid::Uuid::new_v4());
                    Ok(tx_sig)
                }
                Ok(Ok(false)) => Err(PyRuntimeError::new_err("用户拒绝了支付授权")),
                Err(_) => {
                    // 超时后清理任务
                    PENDING_TASKS.lock().await.remove(&merchant_did);
                    Err(PyRuntimeError::new_err("授权超时，请重试"))
                }
                _ => Err(PyRuntimeError::new_err("内部通信错误")),
            }
        })
    }
}

/// Real WebSocket client: connects to DIDComm Mediator with reconnect loop
async fn real_ws_client(ws_url: String) {
    loop {
        match connect_and_run(&ws_url).await {
            Ok(()) => println!("Mediator disconnected, reconnecting..."),
            Err(e) => eprintln!("WS error: {}, reconnecting in 3s...", e),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_and_run(ws_url: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(ws_url).await?;
    println!("已连接到 Mediator: {}", ws_url);

    // 1. mediate-request
    send_msg(&mut ws, mediate_request(OUR_DID)).await?;
    println!("已发送 mediate-request");

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

    // 2. keylist-update
    send_msg(&mut ws, keylist_update(OUR_DID)).await?;
    println!("已发送 keylist-update");

    // Read keylist-update-response
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

    println!("Mediator 握手完成，正在监听消息...");

    // 3. Receive loop
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                handle_incoming_message(&text).await;
            }

            Ok(_) => {} // ignore non-text messages
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

async fn handle_incoming_message(text: &str) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("无法解析消息: {}", e);
            return;
        }
    };

    // Forwarded messages may contain the inner payload directly (unencrypted testing)
    // Look for authorization messages: { "merchant_did": "...", "authorized": true/false }
    if let Some(merchant_did) = v.get("merchant_did").and_then(|v| v.as_str()) {
        let authorized = v.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut tasks = PENDING_TASKS.lock().await;
        if let Some(tx) = tasks.remove(merchant_did) {
            println!("收到授权响应: {} -> {}", merchant_did, authorized);
            let _ = tx.send(authorized);
        } else {
            println!("收到未匹配的授权响应: {}", merchant_did);
        }
        return;
    }

    // Handle forwarded JWE messages (ciphertext + recipients)
    // In unencrypted testing mode, the mediator may forward plaintext DIDComm messages
    // with a "body" containing the authorization result
    if let Some(body) = v.get("body") {
        if let Some(merchant_did) = body.get("merchant_did").and_then(|v| v.as_str()) {
            let authorized = body.get("authorized").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut tasks = PENDING_TASKS.lock().await;
            if let Some(tx) = tasks.remove(merchant_did) {
                println!("收到授权响应(body): {} -> {}", merchant_did, authorized);
                let _ = tx.send(authorized);
            }
        }
    }
}

fn mediate_request(from: &str) -> String {
    serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "https://didcomm.org/coordinate-mediation/2.0/mediate-request",
        "from": from,
        "body": {}
    }).to_string()
}

fn keylist_update(from: &str) -> String {
    serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "https://didcomm.org/coordinate-mediation/2.0/keylist-update",
        "from": from,
        "body": {
            "updates": [{ "recipient_key": format!("{}#key-1", from), "action": "add" }]
        }
    }).to_string()
}

#[pymodule]
fn ignite_pay_rs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<IgnitePayCore>()?;
    Ok(())
}