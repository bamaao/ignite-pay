// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use crate::error::Result;
use borsh::{BorshSerialize, BorshDeserialize};
use serde::{Deserialize, Serialize};
use sled::Db;
use solana_program::hash::hash;
use solana_pubkey::Pubkey;

/// A hub registered in the network for multi-hop routing.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HubLeaf {
    /// Hash of the hub's DID identifier.
    pub hub_did_hash: [u8; 32],
    /// Currently active public key for the hub.
    pub active_pubkey: Pubkey,
    /// Hash of the hub's endpoint URL.
    pub endpoint_hash: [u8; 32],
    /// Collateral staked by the hub.
    pub collateral: u64,
    /// Hash of the hub's platform verifiable credential.
    pub platform_vc_hash: [u8; 32],
    /// Hash of the hub's performance metrics.
    pub metrics_hash: [u8; 32],
    /// Slot when the hub was last updated.
    pub slot_updated: u64,
}

/// Performance and reliability metrics for a hub.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HubMetrics {
    /// Online rate as basis points (10000 = 100%).
    pub online_rate: u16,
    /// Success rate as basis points (10000 = 100%).
    pub success_rate: u16,
    /// Average routing latency in milliseconds.
    pub avg_latency_ms: u32,
    /// Total amount routed through this hub.
    pub total_routed: u64,
    /// Total number of transactions.
    pub total_transactions: u64,
    /// Number of currently active channels.
    pub active_channels: u32,
    /// Available liquidity in the hub.
    pub available_liquidity: u64,
    /// Fee rate in basis points.
    pub fee_rate_bps: u16,
}

/// Manager for hub registration and metrics backed by sled.
pub struct HubManager {
    db: Db,
}

impl std::fmt::Debug for HubManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubManager").finish()
    }
}

impl HubManager {
    /// Create a new HubManager backed by a sled database.
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self { db })
    }

    /// Register a new hub or update an existing one.
    pub fn register_hub(&self, hub: HubLeaf) -> Result<()> {
        let key = format!("hub:{}", hex::encode(hub.hub_did_hash));
        let data = borsh::to_vec(&hub)?;
        self.db.insert(key.as_bytes(), data)?;
        self.db.flush()?;
        Ok(())
    }

    /// Get a hub by its DID hash.
    pub fn get_hub(&self, hub_did_hash: [u8; 32]) -> Result<Option<HubLeaf>> {
        let key = format!("hub:{}", hex::encode(hub_did_hash));
        match self.db.get(key.as_bytes())? {
            Some(data) => {
                let hub: HubLeaf = borsh::from_slice(&data)?;
                Ok(Some(hub))
            }
            None => Ok(None),
        }
    }

    /// Get metrics for a hub.
    pub fn get_metrics(&self, hub_did_hash: [u8; 32]) -> Result<Option<HubMetrics>> {
        let key = format!("hub_metrics:{}", hex::encode(hub_did_hash));
        match self.db.get(key.as_bytes())? {
            Some(data) => {
                let metrics: HubMetrics = borsh::from_slice(&data)?;
                Ok(Some(metrics))
            }
            None => Ok(None),
        }
    }

    /// Update metrics for a hub.
    pub fn update_metrics(&self, hub_did_hash: [u8; 32], metrics: HubMetrics) -> Result<()> {
        let key = format!("hub_metrics:{}", hex::encode(hub_did_hash));
        let data = borsh::to_vec(&metrics)?;
        self.db.insert(key.as_bytes(), data)?;
        self.db.flush()?;
        Ok(())
    }

    /// List all registered hub DID hashes.
    pub fn list_hubs(&self) -> Result<Vec<[u8; 32]>> {
        let prefix = b"hub:";
        let mut hubs = Vec::new();
        for item in self.db.scan_prefix(prefix) {
            let (key, _) = item?;
            // Key format: "hub:{hex(did_hash)}"
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if let Some(hex_part) = key_str.strip_prefix("hub:") {
                    if let Ok(bytes) = hex::decode(hex_part) {
                        if bytes.len() == 32 {
                            let mut did_hash = [0u8; 32];
                            did_hash.copy_from_slice(&bytes);
                            hubs.push(did_hash);
                        }
                    }
                }
            }
        }
        Ok(hubs)
    }

    /// Compute a deterministic hash of hub metrics.
    pub fn compute_metrics_hash(metrics: &HubMetrics) -> [u8; 32] {
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&metrics.online_rate.to_le_bytes());
        data.extend_from_slice(&metrics.success_rate.to_le_bytes());
        data.extend_from_slice(&metrics.avg_latency_ms.to_le_bytes());
        data.extend_from_slice(&metrics.total_routed.to_le_bytes());
        data.extend_from_slice(&metrics.total_transactions.to_le_bytes());
        data.extend_from_slice(&metrics.active_channels.to_le_bytes());
        data.extend_from_slice(&metrics.available_liquidity.to_le_bytes());
        data.extend_from_slice(&metrics.fee_rate_bps.to_le_bytes());
        hash(&data).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn make_hub(did_hash: [u8; 32]) -> HubLeaf {
        HubLeaf {
            hub_did_hash: did_hash,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [1u8; 32],
            collateral: 10_000_000,
            platform_vc_hash: [2u8; 32],
            metrics_hash: [3u8; 32],
            slot_updated: 100,
        }
    }

    fn make_metrics() -> HubMetrics {
        HubMetrics {
            online_rate: 9900,
            success_rate: 9950,
            avg_latency_ms: 50,
            total_routed: 1_000_000_000,
            total_transactions: 5000,
            active_channels: 20,
            available_liquidity: 50_000_000,
            fee_rate_bps: 10,
        }
    }

    #[test]
    fn test_register_and_get_hub() {
        let db = temp_db();
        let mgr = HubManager::new(db).unwrap();
        let did_hash = [42u8; 32];
        let hub = make_hub(did_hash);

        mgr.register_hub(hub.clone()).unwrap();

        let loaded = mgr.get_hub(did_hash).unwrap().unwrap();
        assert_eq!(loaded.hub_did_hash, did_hash);
        assert_eq!(loaded.collateral, 10_000_000);
    }

    #[test]
    fn test_get_nonexistent_hub() {
        let db = temp_db();
        let mgr = HubManager::new(db).unwrap();
        assert!(mgr.get_hub([0u8; 32]).unwrap().is_none());
    }

    #[test]
    fn test_update_and_get_metrics() {
        let db = temp_db();
        let mgr = HubManager::new(db).unwrap();
        let did_hash = [1u8; 32];
        let metrics = make_metrics();

        mgr.update_metrics(did_hash, metrics.clone()).unwrap();

        let loaded = mgr.get_metrics(did_hash).unwrap().unwrap();
        assert_eq!(loaded.online_rate, 9900);
        assert_eq!(loaded.success_rate, 9950);
        assert_eq!(loaded.avg_latency_ms, 50);
        assert_eq!(loaded.fee_rate_bps, 10);
    }

    #[test]
    fn test_list_hubs() {
        let db = temp_db();
        let mgr = HubManager::new(db).unwrap();

        mgr.register_hub(make_hub([1u8; 32])).unwrap();
        mgr.register_hub(make_hub([2u8; 32])).unwrap();
        mgr.register_hub(make_hub([3u8; 32])).unwrap();

        let hubs = mgr.list_hubs().unwrap();
        assert_eq!(hubs.len(), 3);
    }

    #[test]
    fn test_compute_metrics_hash_deterministic() {
        let metrics = make_metrics();
        let h1 = HubManager::compute_metrics_hash(&metrics);
        let h2 = HubManager::compute_metrics_hash(&metrics);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_metrics_hash_differs() {
        let mut metrics1 = make_metrics();
        let metrics2 = make_metrics();
        metrics1.online_rate = 5000;
        let h1 = HubManager::compute_metrics_hash(&metrics1);
        let h2 = HubManager::compute_metrics_hash(&metrics2);
        assert_ne!(h1, h2);
    }
}
