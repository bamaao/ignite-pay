use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Tracks on-chain settlement state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementRecord {
    pub channel_id: [u8; 32],
    pub buyer: [u8; 32],
    pub nonce: u64,
    pub amount: u64,
    pub merkle_root: [u8; 32],
    pub tx_signature: String,
}

/// Sled-backed settlement tracking.
pub struct SettlementStore {
    db: sled::Db,
}

impl SettlementStore {
    pub fn new(db: sled::Db) -> Self {
        Self { db }
    }

    fn settlements_tree(&self) -> Result<sled::Tree> {
        Ok(self.db.open_tree("mb_settlements")?)
    }

    /// Record a new settlement, keyed by `channel_id || nonce` (big-endian).
    pub fn record_settlement(&self, record: &SettlementRecord) -> Result<()> {
        let tree = self.settlements_tree()?;
        let mut key = Vec::with_capacity(40);
        key.extend_from_slice(&record.channel_id);
        key.extend_from_slice(&record.nonce.to_be_bytes());
        let value = serde_json::to_vec(record)?;
        tree.insert(&key, value)?;
        tree.flush()?;
        Ok(())
    }

    /// Get a settlement record by channel and nonce.
    pub fn get_settlement(&self, channel_id: &[u8; 32], nonce: u64) -> Result<Option<SettlementRecord>> {
        let tree = self.settlements_tree()?;
        let mut key = Vec::with_capacity(40);
        key.extend_from_slice(channel_id);
        key.extend_from_slice(&nonce.to_be_bytes());
        match tree.get(&key)? {
            Some(value) => Ok(Some(serde_json::from_slice(&value)?)),
            None => Ok(None),
        }
    }
}
