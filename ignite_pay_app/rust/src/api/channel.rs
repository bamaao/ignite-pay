use anyhow::Result;
use base64::Engine;
use serde::Deserialize;

use crate::api::channel_store::{ChannelInfo, ChannelStore};

// ── Return types ────────────────────────────────────────────────────────────

/// Result of parsing a payment QR code.
#[derive(Debug, Clone)]
pub struct PaymentQrData {
    pub merchant_did: String,
    pub amount: u64,
    pub description: String,
    pub order_id: String,
    pub hub_endpoint: String,
    pub timestamp: i64,
    pub merchant_mb_pubkey: String,
    pub merchant_mediator_url: String,
}

/// Result of a channel payment.
#[derive(Debug, Clone)]
pub struct PaymentResult {
    pub channel_id: String,
    pub sequence: u64,
    pub leaf_index: u32,
    pub new_root: String,
}

/// Result of opening a channel.
#[derive(Debug, Clone)]
pub struct OpenChannelResult {
    pub channel_id: String,
    pub sequence: u64,
    pub current_root: String,
}

/// Detailed channel state info.
#[derive(Debug, Clone)]
pub struct ChannelStateInfo {
    pub channel_id: String,
    pub status: String,
    pub sequence: u64,
    pub leaf_count: u32,
    pub user_balance: u64,
    pub total_deposited: u64,
}

// ── QR Parsing ──────────────────────────────────────────────────────────────

/// Parse a payment QR code string.
/// Accepts `ignite://pay?d=<base64url>` format or raw JSON.
pub fn parse_payment_qr(qr_data: String) -> Result<PaymentQrData> {
    #[derive(Deserialize)]
    struct QrJson {
        #[serde(rename = "type")]
        qr_type: String,
        merchant_did: String,
        amount: u64,
        #[serde(default)]
        description: String,
        order_id: String,
        hub_endpoint: String,
        timestamp: i64,
        #[serde(default)]
        merchant_mb_pubkey: String,
        #[serde(default)]
        merchant_mediator_url: String,
    }

    let json_str = if qr_data.starts_with("ignite://pay?d=") {
        let encoded = &qr_data["ignite://pay?d=".len()..];
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;
        String::from_utf8(bytes)?
    } else if qr_data.starts_with('{') {
        qr_data
    } else {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&qr_data)
            .map_err(|e| anyhow::anyhow!("Not a valid QR format: {}", e))?;
        String::from_utf8(bytes)?
    };

    let data: QrJson = serde_json::from_str(&json_str)?;
    if data.qr_type != "ignite-pay-request" {
        return Err(anyhow::anyhow!("Invalid QR type: {}", data.qr_type));
    }

    Ok(PaymentQrData {
        merchant_did: data.merchant_did,
        amount: data.amount,
        description: data.description,
        order_id: data.order_id,
        hub_endpoint: data.hub_endpoint,
        timestamp: data.timestamp,
        merchant_mb_pubkey: data.merchant_mb_pubkey,
        merchant_mediator_url: data.merchant_mediator_url,
    })
}

// ── Channel Store Operations ────────────────────────────────────────────────

/// List all stored channels.
pub fn list_channels(storage_path: String) -> Result<Vec<ChannelInfo>> {
    let store = ChannelStore::new(&storage_path)?;
    store.list()
}

/// Get channel state info.
pub fn get_channel_state(storage_path: String, channel_id: String) -> Result<ChannelStateInfo> {
    let store = ChannelStore::new(&storage_path)?;
    let info = store
        .get(&channel_id)?
        .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", channel_id))?;

    Ok(ChannelStateInfo {
        channel_id: info.channel_id,
        status: info.status,
        sequence: info.sequence,
        leaf_count: 0,
        user_balance: info.balance,
        total_deposited: info.total_deposited,
    })
}

// ── Async Hub API Operations ────────────────────────────────────────────────

/// Open a channel with a Hub.
pub async fn open_channel(
    storage_path: String,
    hub_endpoint: String,
    deposit: u64,
    tree_depth: u32,
) -> Result<OpenChannelResult> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/channels/open", hub_endpoint.trim_end_matches('/')))
        .json(&serde_json::json!({
            "deposit_amount": deposit,
            "tree_depth": tree_depth,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Open channel failed: {} - {}", status, body));
    }

    let result: serde_json::Value = resp.json().await?;
    let channel_id = result["channel_id"].as_str().unwrap_or("").to_string();
    let sequence = result["sequence"].as_u64().unwrap_or(0);
    let current_root = result["current_root"].as_str().unwrap_or("").to_string();

    // Save to local store
    let store = ChannelStore::new(&storage_path)?;
    let info = ChannelInfo {
        channel_id: channel_id.clone(),
        hub_endpoint: hub_endpoint.clone(),
        user_pubkey: result["user_pubkey"].as_str().unwrap_or("").to_string(),
        provider_pubkey: result["provider_pubkey"].as_str().unwrap_or("").to_string(),
        status: "Open".to_string(),
        sequence,
        balance: deposit,
        total_deposited: deposit,
        tree_depth,
    };
    store.save(&info)?;

    Ok(OpenChannelResult {
        channel_id,
        sequence,
        current_root,
    })
}

/// Pay through a channel.
pub async fn channel_pay(
    storage_path: String,
    channel_id: String,
    hub_endpoint: String,
    amount: u64,
    recipient_pubkey: String,
) -> Result<PaymentResult> {
    let store = ChannelStore::new(&storage_path)?;
    let info = store
        .get(&channel_id)?
        .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", channel_id))?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/v1/channels/{}/pay",
            hub_endpoint.trim_end_matches('/'),
            channel_id
        ))
        .json(&serde_json::json!({
            "amount": amount,
            "new_owner": recipient_pubkey,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Channel pay failed: {} - {}", status, body));
    }

    let result: serde_json::Value = resp.json().await?;
    let new_sequence = result["sequence"].as_u64().unwrap_or(info.sequence + 1);
    let leaf_index = result["leaf_index"].as_u64().unwrap_or(0) as u32;
    let new_root = result["new_root"].as_str().unwrap_or("").to_string();

    // Update local store
    let mut updated = info.clone();
    updated.sequence = new_sequence;
    updated.balance = updated.balance.saturating_sub(amount);
    store.save(&updated)?;

    Ok(PaymentResult {
        channel_id,
        sequence: new_sequence,
        leaf_index,
        new_root,
    })
}

/// Close a channel.
pub async fn close_channel(storage_path: String, channel_id: String) -> Result<String> {
    let store = ChannelStore::new(&storage_path)?;
    let info = store
        .get(&channel_id)?
        .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", channel_id))?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/v1/channels/{}/close",
            info.hub_endpoint.trim_end_matches('/'),
            channel_id
        ))
        .json(&serde_json::json!({}))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Close channel failed: {} - {}", status, body));
    }

    store.update_status(&channel_id, "Closed")?;
    Ok(format!("Channel {} closed.", channel_id))
}

/// Settle a channel (claim + finalize).
pub async fn settle_channel(
    storage_path: String,
    channel_id: String,
    hub_endpoint: String,
) -> Result<String> {
    let client = reqwest::Client::new();

    // Initiate settlement
    let resp = client
        .post(format!(
            "{}/v1/channels/{}/settle",
            hub_endpoint.trim_end_matches('/'),
            channel_id
        ))
        .json(&serde_json::json!({}))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Settle failed: {} - {}", status, body));
    }

    let store = ChannelStore::new(&storage_path)?;
    store.update_status(&channel_id, "Settling")?;

    Ok(format!("Channel {} settlement initiated.", channel_id))
}
