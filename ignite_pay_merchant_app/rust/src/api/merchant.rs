use anyhow::Result;
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek_v1::Keypair as V1Keypair;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

// ── Global state ────────────────────────────────────────────────────────

static GLOBAL_DB: Lazy<Mutex<Option<sled::Db>>> = Lazy::new(|| Mutex::new(None));
static GLOBAL_KEYPAIR: Lazy<Mutex<Option<[u8; 64]>>> = Lazy::new(|| Mutex::new(None));

// ── Bridge return types ─────────────────────────────────────────────────

pub struct DidInfo {
    pub did: String,
    pub did_doc_json: String,
}

pub struct PaymentOrderBridge {
    pub order_id: String,
    pub merchant_did: String,
    pub amount: u64,
    pub description: String,
    pub hub_endpoint: String,
    pub status: String,
    pub created_at: i64,
    pub confirmed_at: Option<i64>,
    pub channel_id: Option<String>,
    pub leaf_index: Option<u32>,
    pub sequence: Option<u64>,
}

pub struct ChannelStatusBridge {
    pub channel_id: String,
    pub status: String,
    pub sequence: u64,
    pub leaf_count: u32,
    pub provider_balance: u64,
    pub total_deposited: u64,
}

pub struct AuditEntryBridge {
    pub timestamp: i64,
    pub event_type: String,
    pub order_id: Option<String>,
    pub amount: Option<u64>,
    pub detail: String,
}

// ── Internal types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum OrderStatus {
    Pending,
    Confirmed,
    Failed,
    Expired,
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderStatus::Pending => write!(f, "pending"),
            OrderStatus::Confirmed => write!(f, "confirmed"),
            OrderStatus::Failed => write!(f, "failed"),
            OrderStatus::Expired => write!(f, "expired"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaymentOrder {
    order_id: String,
    merchant_did: String,
    amount: u64,
    description: String,
    hub_endpoint: String,
    status: OrderStatus,
    created_at: DateTime<Utc>,
    confirmed_at: Option<DateTime<Utc>>,
    channel_id: Option<String>,
    leaf_index: Option<u32>,
    sequence: Option<u64>,
}

impl PaymentOrder {
    fn to_bridge(&self) -> PaymentOrderBridge {
        PaymentOrderBridge {
            order_id: self.order_id.clone(),
            merchant_did: self.merchant_did.clone(),
            amount: self.amount,
            description: self.description.clone(),
            hub_endpoint: self.hub_endpoint.clone(),
            status: self.status.to_string(),
            created_at: self.created_at.timestamp(),
            confirmed_at: self.confirmed_at.map(|t| t.timestamp()),
            channel_id: self.channel_id.clone(),
            leaf_index: self.leaf_index,
            sequence: self.sequence,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditEntry {
    timestamp: DateTime<Utc>,
    event_type: String,
    order_id: Option<String>,
    amount: Option<u64>,
    detail: String,
}

// ── QR types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaymentQrData {
    #[serde(rename = "type")]
    qr_type: String,
    version: u32,
    merchant_did: String,
    amount: u64,
    #[serde(default)]
    description: String,
    order_id: String,
    hub_endpoint: String,
    timestamp: i64,
    #[serde(default)]
    merchant_mb_pubkey: String,
}

// ── Helper: open sled trees ─────────────────────────────────────────────

fn open_db(storage_path: &str) -> Result<sled::Db> {
    Ok(sled::open(storage_path)?)
}

fn orders_tree(db: &sled::Db) -> Result<sled::Tree> {
    db.open_tree("orders")
        .map_err(|e| anyhow::anyhow!("Failed to open orders tree: {}", e))
}

fn audit_tree(db: &sled::Db) -> Result<sled::Tree> {
    db.open_tree("merchant_audit")
        .map_err(|e| anyhow::anyhow!("Failed to open audit tree: {}", e))
}

fn keypair_tree(db: &sled::Db) -> Result<sled::Tree> {
    db.open_tree("keypairs")
        .map_err(|e| anyhow::anyhow!("Failed to open keypairs tree: {}", e))
}

// ── Identity functions ──────────────────────────────────────────────────

/// Initialize merchant identity - generates or loads DID from storage.
pub fn initialize_merchant(storage_path: String) -> Result<DidInfo> {
    let db = open_db(&storage_path)?;

    // Generate or load keypair
    let kp_tree = keypair_tree(&db)?;
    let keypair_bytes: [u8; 64] = if let Some(bytes) = kp_tree.get("merchant_keypair")? {
        bytes.as_ref().try_into().map_err(|_| anyhow::anyhow!("Invalid keypair bytes"))?
    } else {
        let kp = ignite_pay_state_channel::signing::generate_keypair();
        let bytes = kp.to_bytes();
        kp_tree.insert("merchant_keypair", &bytes[..])?;
        kp_tree.flush()?;
        bytes
    };

    // Build DID from public key
    let kp = V1Keypair::from_bytes(&keypair_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair: {}", e))?;
    let pubkey_b58 = bs58::encode(kp.public.to_bytes()).into_string();
    let did = format!("did:ignite:{}", pubkey_b58);

    // Build a minimal DID document
    let did_doc = serde_json::json!({
        "@context": ["https://www.w3.org/ns/did/v1"],
        "id": &did,
        "verificationMethod": [{
            "id": format!("{}#key-1", &did),
            "type": "Ed25519VerificationKey2018",
            "controller": &did,
            "publicKeyBase58": pubkey_b58,
        }],
        "authentication": [format!("{}#key-1", &did)],
    });
    let did_doc_json = serde_json::to_string(&did_doc)?;

    // Store in global state
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        {
            let mut global_db = GLOBAL_DB.lock().await;
            *global_db = Some(db);
        }
        {
            let mut global_kp = GLOBAL_KEYPAIR.lock().await;
            *global_kp = Some(keypair_bytes);
        }
    });

    Ok(DidInfo { did, did_doc_json })
}

/// Get the merchant DID from storage.
pub fn get_merchant_did(storage_path: String) -> Result<String> {
    let db = open_db(&storage_path)?;
    let kp_tree = keypair_tree(&db)?;

    let bytes = kp_tree
        .get("merchant_keypair")?
        .ok_or_else(|| anyhow::anyhow!("No merchant keypair found"))?;

    let keypair_bytes: [u8; 64] = bytes.as_ref().try_into().map_err(|_| anyhow::anyhow!("Invalid keypair bytes"))?;
    let kp = V1Keypair::from_bytes(&keypair_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair: {}", e))?;
    let pubkey_b58 = bs58::encode(kp.public.to_bytes()).into_string();
    Ok(format!("did:ignite:{}", pubkey_b58))
}

// ── Keypair management ──────────────────────────────────────────────────

/// Generate a merchant keypair, returns base58-encoded public key.
pub fn generate_merchant_keypair(storage_path: String) -> Result<String> {
    let db = open_db(&storage_path)?;
    let kp_tree = keypair_tree(&db)?;

    let kp = ignite_pay_state_channel::signing::generate_keypair();
    let bytes = kp.to_bytes();
    kp_tree.insert("merchant_keypair", &bytes[..])?;
    kp_tree.flush()?;

    let kp_v1 = V1Keypair::from_bytes(&bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair: {}", e))?;
    let pubkey_b58 = bs58::encode(kp_v1.public.to_bytes()).into_string();

    // Update global state
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        {
            let mut global_kp = GLOBAL_KEYPAIR.lock().await;
            *global_kp = Some(bytes);
        }
        {
            let mut global_db = GLOBAL_DB.lock().await;
            *global_db = Some(db);
        }
    });

    Ok(pubkey_b58)
}

/// Get the stored merchant public key as base58.
pub fn get_merchant_pubkey(storage_path: String) -> Result<String> {
    let db = open_db(&storage_path)?;
    let kp_tree = keypair_tree(&db)?;

    let bytes = kp_tree
        .get("merchant_keypair")?
        .ok_or_else(|| anyhow::anyhow!("No merchant keypair found"))?;

    let keypair_bytes: [u8; 64] = bytes.as_ref().try_into().map_err(|_| anyhow::anyhow!("Invalid keypair bytes"))?;
    let kp = V1Keypair::from_bytes(&keypair_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair: {}", e))?;
    Ok(bs58::encode(kp.public.to_bytes()).into_string())
}

// ── QR generation ───────────────────────────────────────────────────────

/// Generate a payment QR string and create an order.
/// Format: `ignite://pay?d=<base64url(json)>`
pub fn generate_payment_qr(
    merchant_did: String,
    amount: u64,
    description: String,
    hub_endpoint: String,
    merchant_mb_pubkey: String,
) -> Result<String> {
    let order_id = uuid::Uuid::new_v4().to_string();
    let timestamp = Utc::now().timestamp();

    let qr_data = PaymentQrData {
        qr_type: "ignite-pay-request".to_string(),
        version: 1,
        merchant_did: merchant_did.clone(),
        amount,
        description: description.clone(),
        order_id: order_id.clone(),
        hub_endpoint: hub_endpoint.clone(),
        timestamp,
        merchant_mb_pubkey,
    };

    let json = serde_json::to_string(&qr_data)?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes());
    let qr_text = format!("ignite://pay?d={}", encoded);

    // Create a pending order in storage
    let order = PaymentOrder {
        order_id,
        merchant_did,
        amount,
        description,
        hub_endpoint,
        status: OrderStatus::Pending,
        created_at: Utc::now(),
        confirmed_at: None,
        channel_id: None,
        leaf_index: None,
        sequence: None,
    };

    // Try to save order (best effort - storage may not be initialized yet)
    if let Ok(db) = open_db_from_global().or_else(|_| sled::open("merchant_data")) {
        if let Ok(tree) = orders_tree(&db) {
            let value = serde_json::to_vec(&order)?;
            let _ = tree.insert(order.order_id.as_bytes(), value);
            let _ = tree.flush();
        }
    }

    Ok(qr_text)
}

/// Generate an ASCII QR code for debugging.
pub fn generate_qr_ascii(qr_text: String) -> Result<String> {
    let code = qrcode::QrCode::new(qr_text.as_bytes())
        .map_err(|e| anyhow::anyhow!("QR generation failed: {}", e))?;
    Ok(code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build())
}

fn open_db_from_global() -> Result<sled::Db> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let global = GLOBAL_DB.lock().await;
        global
            .clone()
            .ok_or_else(|| anyhow::anyhow!("DB not initialized"))
    })
}

// ── Order management ────────────────────────────────────────────────────

/// List recent payment orders.
pub fn list_orders(storage_path: String, limit: u32) -> Result<Vec<PaymentOrderBridge>> {
    let db = open_db(&storage_path)?;
    let tree = orders_tree(&db)?;
    let mut orders = Vec::new();
    for item in tree.iter().rev() {
        if orders.len() >= limit as usize {
            break;
        }
        let (_, value) = item?;
        let order: PaymentOrder = serde_json::from_slice(&value)?;
        orders.push(order.to_bridge());
    }
    Ok(orders)
}

/// Get a single payment order by ID.
pub fn get_order(storage_path: String, order_id: String) -> Result<Option<PaymentOrderBridge>> {
    let db = open_db(&storage_path)?;
    let tree = orders_tree(&db)?;
    if let Some(bytes) = tree.get(order_id.as_bytes())? {
        let order: PaymentOrder = serde_json::from_slice(&bytes)?;
        Ok(Some(order.to_bridge()))
    } else {
        Ok(None)
    }
}

/// Confirm a payment order with channel metadata.
pub fn confirm_order(
    storage_path: String,
    order_id: String,
    channel_id: String,
    leaf_index: u32,
    sequence: u64,
) -> Result<()> {
    let db = open_db(&storage_path)?;
    let tree = orders_tree(&db)?;

    if let Some(bytes) = tree.get(order_id.as_bytes())? {
        let mut order: PaymentOrder = serde_json::from_slice(&bytes)?;
        order.status = OrderStatus::Confirmed;
        order.confirmed_at = Some(Utc::now());
        order.channel_id = Some(channel_id);
        order.leaf_index = Some(leaf_index);
        order.sequence = Some(sequence);
        let value = serde_json::to_vec(&order)?;
        tree.insert(order_id.as_bytes(), value)?;
        tree.flush()?;
    }

    Ok(())
}

/// Get all pending (unconfirmed) orders.
pub fn get_pending_orders(storage_path: String) -> Result<Vec<PaymentOrderBridge>> {
    let db = open_db(&storage_path)?;
    let tree = orders_tree(&db)?;
    let mut orders = Vec::new();
    for item in tree.iter().rev() {
        let (_, value) = item?;
        let order: PaymentOrder = serde_json::from_slice(&value)?;
        if order.status == OrderStatus::Pending {
            orders.push(order.to_bridge());
        }
    }
    Ok(orders)
}

// ── Channel operations (Provider role) ──────────────────────────────────

/// List all state channel IDs.
pub fn merchant_list_channels(storage_path: String) -> Result<Vec<String>> {
    let db = open_db(&storage_path)?;
    let channel_manager = ignite_pay_state_channel::channel::ChannelManager::new(db)?;
    let channel_db = channel_manager.db();

    let mut channels = Vec::new();
    for item in channel_db.iter() {
        let (key, _) = item?;
        if key.len() == 32 {
            channels.push(hex::encode(&key));
        }
    }
    Ok(channels)
}

/// Get channel status.
pub fn merchant_get_channel_status(
    storage_path: String,
    channel_id: String,
) -> Result<ChannelStatusBridge> {
    let db = open_db(&storage_path)?;
    let channel_manager = ignite_pay_state_channel::channel::ChannelManager::new(db)?;

    let bytes = parse_channel_id(&channel_id)?;
    let state = channel_manager.load_state(&bytes)?;

    // Get keypair for provider balance calculation
    let kp_bytes = get_keypair_from_storage(&storage_path)?;
    let kp = V1Keypair::from_bytes(&kp_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair: {}", e))?;
    let provider_pk = ignite_pay_state_channel::signing::to_pubkey(&kp);

    let provider_balance: u64 = state
        .tree
        .leaves()
        .iter()
        .filter(|l| l.owner == provider_pk)
        .map(|l| l.amount)
        .sum();

    Ok(ChannelStatusBridge {
        channel_id,
        status: format!("{:?}", state.metadata.status),
        sequence: state.metadata.sequence,
        leaf_count: state.metadata.leaf_count,
        provider_balance,
        total_deposited: state.metadata.total_deposited,
    })
}

/// Cooperative close a channel.
pub async fn merchant_close_channel(
    storage_path: String,
    hub_endpoint: String,
    channel_id: String,
) -> Result<String> {
    let http = reqwest::Client::new();
    let endpoint = hub_endpoint.trim_end_matches('/');

    let resp = http
        .post(format!("{}/v1/channels/{}/close", endpoint, channel_id))
        .json(&serde_json::json!({}))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Close failed: {} - {}", status, body));
    }

    // Update local state to Closed
    let db = open_db(&storage_path)?;
    let channel_manager = ignite_pay_state_channel::channel::ChannelManager::new(db)?;
    if let Ok(bytes) = parse_channel_id(&channel_id) {
        if let Ok(mut state) = channel_manager.load_state(&bytes) {
            state.metadata.status = ignite_pay_state_channel::types::ChannelStatus::Closed;
            let _ = channel_manager.persist_state(&state);
        }
    }

    Ok(format!("Channel {} closed.", channel_id))
}

/// Claim a leaf during settlement.
pub async fn merchant_claim_leaf(
    _storage_path: String,
    hub_endpoint: String,
    channel_id: String,
    leaf_index: u32,
    amount: u64,
) -> Result<String> {
    let http = reqwest::Client::new();
    let endpoint = hub_endpoint.trim_end_matches('/');

    let resp = http
        .post(format!("{}/v1/channels/{}/claim", endpoint, channel_id))
        .json(&serde_json::json!({
            "leaf_index": leaf_index,
            "claim_amount": amount,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Claim failed: {} - {}", status, body));
    }

    Ok(format!(
        "Claimed leaf {} for {} from channel {}.",
        leaf_index, amount, channel_id
    ))
}

/// Finalize settlement.
pub async fn merchant_finalize(
    _storage_path: String,
    hub_endpoint: String,
    channel_id: String,
) -> Result<String> {
    let http = reqwest::Client::new();
    let endpoint = hub_endpoint.trim_end_matches('/');

    let resp = http
        .post(format!("{}/v1/channels/{}/finalize", endpoint, channel_id))
        .json(&serde_json::json!({}))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Finalize failed: {} - {}", status, body));
    }

    Ok(format!("Channel {} finalized.", channel_id))
}

// ── Audit log ───────────────────────────────────────────────────────────

/// Append an audit log entry.
pub fn append_audit(
    storage_path: String,
    event_type: String,
    order_id: Option<String>,
    amount: Option<u64>,
    detail: String,
) -> Result<()> {
    let db = open_db(&storage_path)?;
    let tree = audit_tree(&db)?;

    let entry = AuditEntry {
        timestamp: Utc::now(),
        event_type,
        order_id,
        amount,
        detail,
    };
    let key = format!("{}:{:09}", entry.timestamp.timestamp_micros(), tree.len());
    let value = serde_json::to_vec(&entry)?;
    tree.insert(key.as_bytes(), value)?;
    tree.flush()?;

    Ok(())
}

/// Get recent audit entries.
pub fn recent_audit(storage_path: String, limit: u32) -> Result<Vec<AuditEntryBridge>> {
    let db = open_db(&storage_path)?;
    let tree = audit_tree(&db)?;

    let mut entries = Vec::new();
    for item in tree.iter().rev() {
        if entries.len() >= limit as usize {
            break;
        }
        let (_, value) = item?;
        let entry: AuditEntry = serde_json::from_slice(&value)?;
        entries.push(AuditEntryBridge {
            timestamp: entry.timestamp.timestamp(),
            event_type: entry.event_type,
            order_id: entry.order_id,
            amount: entry.amount,
            detail: entry.detail,
        });
    }
    Ok(entries)
}

/// Initialize or load MB merchant keypair from sled, return base58 pubkey.
pub fn initialize_mb_merchant(storage_path: String) -> Result<String> {
    crate::api::merchant_mb::initialize_mb_merchant(storage_path)
}

/// Get the MB merchant pubkey from storage.
pub fn get_mb_merchant_pubkey(storage_path: String) -> Result<String> {
    crate::api::merchant_mb::get_mb_merchant_pubkey(storage_path)
}

// ── Internal helpers ────────────────────────────────────────────────────

fn parse_channel_id(hex_str: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| anyhow::anyhow!("Invalid channel ID hex: {}", e))?;
    bytes
        .try_into()
        .map_err(|_: Vec<u8>| anyhow::anyhow!("Channel ID must be 32 bytes"))
}

fn get_keypair_from_storage(storage_path: &str) -> Result<[u8; 64]> {
    let db = sled::open(storage_path)?;
    let kp_tree = keypair_tree(&db)?;
    let bytes = kp_tree
        .get("merchant_keypair")?
        .ok_or_else(|| anyhow::anyhow!("No merchant keypair found"))?;
    bytes
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid keypair bytes"))
}
