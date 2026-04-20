use anyhow::Result;
use ed25519_dalek_v1::Keypair as V1Keypair;
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, sign_state, to_pubkey};
use ignite_pay_state_channel::types::{ChannelStatus, LeafUpdate};
use serde::{Deserialize, Serialize};

/// Merchant-side state channel client.
/// The merchant acts as the Provider role: receives payments, co-signs states, settles.
pub struct MerchantChannelClient {
    http: reqwest::Client,
    hub_endpoint: String,
    channel_manager: ChannelManager,
    keypair_bytes: [u8; 64],
}

impl MerchantChannelClient {
    pub fn new(hub_endpoint: &str, db: sled::Db, keypair_bytes: &[u8; 64]) -> Result<Self> {
        let channel_manager = ChannelManager::new(db)?;
        Ok(Self {
            http: reqwest::Client::new(),
            hub_endpoint: hub_endpoint.trim_end_matches('/').to_string(),
            channel_manager,
            keypair_bytes: *keypair_bytes,
        })
    }

    /// Generate a new random keypair for channel operations.
    pub fn generate_keypair() -> [u8; 64] {
        generate_keypair().to_bytes()
    }

    /// Get the merchant's pubkey as base58.
    pub fn pubkey(&self) -> String {
        let kp = self.v1_keypair();
        to_pubkey(&kp).to_string()
    }

    fn v1_keypair(&self) -> V1Keypair {
        V1Keypair::from_bytes(&self.keypair_bytes)
            .expect("Stored keypair bytes are always valid")
    }

    /// Fund a channel as the Provider (counterparty deposit).
    pub async fn fund_channel(
        &self,
        channel_id_hex: &str,
        source_vault: &str,
        deposit_b: u64,
    ) -> Result<String> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/fund",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({
                "source_vault": source_vault,
                "deposit_b": deposit_b,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Fund channel failed: {} - {}", status, body));
        }

        Ok(format!("Channel {} funded with {} tokens.", channel_id_hex, deposit_b))
    }

    /// Accept a payment (leaf update) from the user side.
    pub async fn accept_payment(
        &self,
        channel_id_hex: &str,
        update: &LeafUpdate,
    ) -> Result<AcceptPaymentResult> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/accept-payment",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({
                "update": {
                    "channel_id": hex::encode(update.channel_id),
                    "sequence": update.sequence,
                    "leaf_index": update.leaf_index,
                    "prev_leaf_hash": hex::encode(update.prev_leaf_hash),
                    "new_leaf": update.new_leaf,
                    "signature": hex::encode(update.signature),
                },
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Accept payment failed: {} - {}", status, body));
        }

        let result: serde_json::Value = resp.json().await?;
        Ok(AcceptPaymentResult {
            channel_id: result["channel_id"].as_str().unwrap_or("").to_string(),
            sequence: result["sequence"].as_u64().unwrap_or(0),
            new_root: result["new_root"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Co-sign a state (provide provider signature).
    pub fn cosign_state(
        &self,
        channel_id: &[u8; 32],
        sequence: u64,
        root: &[u8; 32],
    ) -> [u8; 64] {
        let kp = self.v1_keypair();
        sign_state(channel_id, sequence, root, &kp)
    }

    /// Get channel status from local store.
    pub fn get_channel_status(&self, channel_id_hex: &str) -> Result<ChannelStatusResult> {
        let bytes = Self::parse_channel_id(channel_id_hex)?;
        let state = self.channel_manager.load_state(&bytes)?;
        let kp = self.v1_keypair();
        let provider_pk = to_pubkey(&kp);

        let provider_balance: u64 = state
            .tree
            .leaves()
            .iter()
            .filter(|l| l.owner == provider_pk)
            .map(|l| l.amount)
            .sum();

        Ok(ChannelStatusResult {
            channel_id: channel_id_hex.to_string(),
            status: format!("{:?}", state.metadata.status),
            sequence: state.metadata.sequence,
            leaf_count: state.metadata.leaf_count,
            provider_balance,
            total_deposited: state.metadata.total_deposited,
        })
    }

    /// List all channels.
    pub fn list_channels(&self) -> Result<Vec<String>> {
        let db = self.channel_manager.db();
        let mut channels = Vec::new();
        for item in db.iter() {
            let (key, _) = item?;
            if key.len() == 32 {
                channels.push(hex::encode(&key));
            }
        }
        Ok(channels)
    }

    /// Cooperative close.
    pub async fn close_channel(&self, channel_id_hex: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/close",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Close failed: {} - {}", status, body));
        }

        if let Ok(bytes) = Self::parse_channel_id(channel_id_hex) {
            if let Ok(mut state) = self.channel_manager.load_state(&bytes) {
                state.metadata.status = ChannelStatus::Closed;
                let _ = self.channel_manager.persist_state(&state);
            }
        }

        Ok(format!("Channel {} closed.", channel_id_hex))
    }

    /// Claim a leaf during settlement.
    pub async fn claim_leaf(
        &self,
        channel_id_hex: &str,
        leaf_index: u32,
        claim_amount: u64,
    ) -> Result<String> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/claim",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({
                "leaf_index": leaf_index,
                "claim_amount": claim_amount,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Claim failed: {} - {}", status, body));
        }

        Ok(format!("Claimed leaf {} for {} from channel {}.", leaf_index, claim_amount, channel_id_hex))
    }

    /// Finalize settlement.
    pub async fn finalize(&self, channel_id_hex: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/finalize",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Finalize failed: {} - {}", status, body));
        }

        Ok(format!("Channel {} finalized.", channel_id_hex))
    }

    fn parse_channel_id(hex_str: &str) -> Result<[u8; 32]> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| anyhow::anyhow!("Invalid channel ID hex: {}", e))?;
        bytes
            .try_into()
            .map_err(|_: Vec<u8>| anyhow::anyhow!("Channel ID must be 32 bytes"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptPaymentResult {
    pub channel_id: String,
    pub sequence: u64,
    pub new_root: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelStatusResult {
    pub channel_id: String,
    pub status: String,
    pub sequence: u64,
    pub leaf_count: u32,
    pub provider_balance: u64,
    pub total_deposited: u64,
}
