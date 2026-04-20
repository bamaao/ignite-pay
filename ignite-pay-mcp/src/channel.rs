use anyhow::Result;
use ed25519_dalek_v1::Keypair as V1Keypair;
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::signing::{generate_keypair, sign_leaf_update, to_pubkey};
use ignite_pay_state_channel::types::{ChannelStatus, UTXOLeaf};
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};

/// Client for interacting with a Hub's state channel User API.
/// Stores the keypair as raw bytes to avoid ed25519-dalek version conflicts
/// (state-channel uses v1, other crates use v2).
pub struct ChannelClient {
    http: reqwest::Client,
    hub_endpoint: String,
    channel_manager: ChannelManager,
    keypair_bytes: [u8; 64],
}

impl ChannelClient {
    /// Create a new channel client.
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
        let kp = generate_keypair();
        kp.to_bytes()
    }

    /// Get the pubkey of this client as a base58 string.
    pub fn pubkey(&self) -> String {
        let kp = self.v1_keypair();
        to_pubkey(&kp).to_string()
    }

    /// Reconstruct the v1 Keypair from stored bytes.
    fn v1_keypair(&self) -> V1Keypair {
        V1Keypair::from_bytes(&self.keypair_bytes)
            .expect("Stored keypair bytes are always valid")
    }

    /// Open a channel with a Hub by calling the Hub's User API.
    pub async fn open_channel(
        &self,
        provider_pubkey: &str,
        token_mint: &str,
        deposit: u64,
        tree_depth: u32,
    ) -> Result<OpenChannelResult> {
        let user_pubkey = self.pubkey();
        let resp = self
            .http
            .post(format!("{}/v1/channels/open", self.hub_endpoint))
            .json(&serde_json::json!({
                "user_pubkey": user_pubkey,
                "provider_pubkey": provider_pubkey,
                "token_mint": token_mint,
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
        let channel_id_hex = result["channel_id"].as_str().unwrap_or("").to_string();
        let sequence = result["sequence"].as_u64().unwrap_or(0);
        let current_root = result["current_root"].as_str().unwrap_or("").to_string();

        // Create locally mirrored state
        let _channel_id_bytes = Self::parse_channel_id(&channel_id_hex)?;
        let kp = self.v1_keypair();
        let user_pk = to_pubkey(&kp);
        let provider_pk: Pubkey = provider_pubkey
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid provider pubkey: {}", e))?;
        let mint_pk: Pubkey = token_mint
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid token mint: {}", e))?;

        let state = self.channel_manager.open_channel(
            &user_pk,
            &provider_pk,
            &mint_pk,
            deposit,
            tree_depth,
            0,
            &user_pk,
            &provider_pk,
            150,
            50,
            None,
        )?;
        self.channel_manager.persist_state(&state)?;

        Ok(OpenChannelResult {
            channel_id: channel_id_hex,
            sequence,
            current_root,
        })
    }

    /// Get channel status from the local store.
    pub fn get_channel_status(&self, channel_id_hex: &str) -> Result<ChannelStatusResult> {
        let channel_id_bytes = Self::parse_channel_id(channel_id_hex)?;
        let state = self.channel_manager.load_state(&channel_id_bytes)?;
        let kp = self.v1_keypair();
        let user_pk = to_pubkey(&kp);

        let user_balance: u64 = state
            .tree
            .leaves()
            .iter()
            .filter(|l| l.owner == user_pk)
            .map(|l| l.amount)
            .sum();

        Ok(ChannelStatusResult {
            channel_id: channel_id_hex.to_string(),
            status: format!("{:?}", state.metadata.status),
            sequence: state.metadata.sequence,
            leaf_count: state.metadata.leaf_count,
            user_balance,
            total_deposited: state.metadata.total_deposited,
        })
    }

    /// List all channels in local storage.
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

    /// Pay through a channel via the Hub's User API.
    pub async fn channel_pay(
        &self,
        channel_id_hex: &str,
        amount: u64,
        recipient_pubkey: &str,
    ) -> Result<PaymentResult> {
        let channel_id_bytes = Self::parse_channel_id(channel_id_hex)?;
        let state = self.channel_manager.load_state(&channel_id_bytes)?;
        let kp = self.v1_keypair();
        let user_pk = to_pubkey(&kp);

        // Find a leaf owned by us with sufficient balance
        let leaf_index = state
            .tree
            .leaves()
            .iter()
            .position(|l| l.owner == user_pk && l.amount >= amount)
            .ok_or_else(|| anyhow::anyhow!("No leaf with sufficient balance found"))?;

        let prev_leaf = state.tree.leaves()[leaf_index].clone();
        let recipient_pk: Pubkey = recipient_pubkey
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid recipient pubkey: {}", e))?;

        let new_leaf = UTXOLeaf::standard(recipient_pk, amount);
        let new_sequence = state.metadata.sequence + 1;

        let _update = sign_leaf_update(
            &state.metadata.channel_id,
            new_sequence,
            leaf_index as u32,
            &prev_leaf,
            new_leaf,
            &kp,
        );

        // Send payment to Hub
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/pay",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({
                "leaf_index": leaf_index,
                "new_owner": recipient_pubkey,
                "amount": amount,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Channel pay failed: {} - {}", status, body));
        }

        let result: serde_json::Value = resp.json().await?;

        Ok(PaymentResult {
            channel_id: channel_id_hex.to_string(),
            sequence: result["sequence"].as_u64().unwrap_or(new_sequence),
            leaf_index: leaf_index as u32,
            new_root: result["new_root"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Close a channel cooperatively via the Hub API.
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
            return Err(anyhow::anyhow!("Close channel failed: {} - {}", status, body));
        }

        // Update local state
        if let Ok(channel_id_bytes) = Self::parse_channel_id(channel_id_hex) {
            if let Ok(mut state) = self.channel_manager.load_state(&channel_id_bytes) {
                state.metadata.status = ChannelStatus::Closed;
                let _ = self.channel_manager.persist_state(&state);
            }
        }

        Ok(format!("Channel {} closed.", channel_id_hex))
    }

    /// Settle a channel via the Hub API.
    pub async fn settle_channel(&self, channel_id_hex: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!(
                "{}/v1/channels/{}/settle",
                self.hub_endpoint, channel_id_hex
            ))
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Settle failed: {} - {}", status, body));
        }

        Ok(format!(
            "Channel {} settlement initiated. Use claim + finalize to complete.",
            channel_id_hex
        ))
    }

    /// Check if there is an open channel available.
    pub fn has_open_channel(&self) -> bool {
        self.get_open_channel_id().is_some()
    }

    /// Get the first open channel ID, if any.
    pub fn get_open_channel_id(&self) -> Option<String> {
        let db = self.channel_manager.db();
        for item in db.iter() {
            let (key, _) = match item {
                Ok(kv) => kv,
                Err(_) => continue,
            };
            if key.len() == 32 {
                if let Ok(channel_id) = <[u8; 32]>::try_from(&key[..]) {
                    if let Ok(state) = self.channel_manager.load_state(&channel_id) {
                        if state.metadata.status == ChannelStatus::Open {
                            return Some(hex::encode(channel_id));
                        }
                    }
                }
            }
        }
        None
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
pub struct OpenChannelResult {
    pub channel_id: String,
    pub sequence: u64,
    pub current_root: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelStatusResult {
    pub channel_id: String,
    pub status: String,
    pub sequence: u64,
    pub leaf_count: u32,
    pub user_balance: u64,
    pub total_deposited: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentResult {
    pub channel_id: String,
    pub sequence: u64,
    pub leaf_index: u32,
    pub new_root: String,
}
