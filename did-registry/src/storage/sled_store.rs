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

/// Sled-based persistent storage for merchant DID records.
pub struct MerchantStore {
    db: sled::Db,
}

impl MerchantStore {
    pub fn new(db: sled::Db) -> Self {
        Self { db }
    }

    /// Save a merchant record keyed by DID hash.
    pub fn save_merchant(&self, did_hash: &[u8], data: &[u8]) -> anyhow::Result<()> {
        let key = format!("merchant:{}", hex::encode(did_hash));
        self.db.insert(key, data)?;
        Ok(())
    }

    /// Get a merchant record by DID hash.
    pub fn get_merchant(&self, did_hash: &[u8]) -> Option<Vec<u8>> {
        let key = format!("merchant:{}", hex::encode(did_hash));
        self.db.get(key).ok().flatten().map(|ivec| ivec.to_vec())
    }

    /// Save a leaf index mapping: did_hash -> leaf_index.
    pub fn save_leaf_index(&self, did_hash: &[u8], leaf_index: u32) -> anyhow::Result<()> {
        let key = format!("leaf_index:{}", hex::encode(did_hash));
        self.db.insert(key, &leaf_index.to_le_bytes())?;
        Ok(())
    }

    /// Get the leaf index for a DID hash.
    pub fn get_leaf_index(&self, did_hash: &[u8]) -> Option<u32> {
        let key = format!("leaf_index:{}", hex::encode(did_hash));
        let bytes = self.db.get(key).ok().flatten()?;
        if bytes.len() == 4 {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&bytes);
            Some(u32::from_le_bytes(arr))
        } else {
            None
        }
    }

    /// Save a verifiable credential keyed by its SHA-256 hash (hex-encoded).
    pub fn save_vc(&self, vc_hash_hex: &str, vc_json: &[u8]) -> anyhow::Result<()> {
        let key = format!("vc:{}", vc_hash_hex);
        self.db.insert(key, vc_json)?;
        Ok(())
    }

    /// Get a verifiable credential by its SHA-256 hash (hex-encoded).
    pub fn get_vc(&self, vc_hash_hex: &str) -> Option<Vec<u8>> {
        let key = format!("vc:{}", vc_hash_hex);
        self.db.get(key).ok().flatten().map(|ivec| ivec.to_vec())
    }

    /// Record a fee entry for an on-chain operation.
    /// Key format: `fee:{operation}:{timestamp_ms}:{did_hash_hex}`
    pub fn record_fee(
        &self,
        did_hash: &[u8],
        operation: &str,
        fee_lamports: u64,
        mode: &str,
        merchant_did: &str,
    ) -> anyhow::Result<()> {
        let timestamp_ms = chrono::Utc::now().timestamp_millis();
        let key = format!(
            "fee:{}:{}:{}",
            operation,
            timestamp_ms,
            hex::encode(did_hash)
        );
        let value = serde_json::json!({
            "merchant_did": merchant_did,
            "operation": operation,
            "fee_lamports": fee_lamports,
            "timestamp": timestamp_ms,
            "mode": mode,
        });
        self.db.insert(key, serde_json::to_vec(&value)?)?;
        Ok(())
    }

    /// List fee records matching a prefix, up to `limit` entries.
    pub fn list_fees(&self, prefix: &str, limit: usize) -> Vec<serde_json::Value> {
        let mut results = Vec::new();
        for item in self.db.scan_prefix(prefix).flatten().take(limit) {
            if let Ok(val) = serde_json::from_slice(&item.1) {
                results.push(val);
            }
        }
        results
    }

    /// Mark a VC as revoked. Stores revocation metadata keyed by vc_hash.
    pub fn mark_vc_revoked(
        &self,
        vc_hash_hex: &str,
        credential_subject_pk: &str,
        reason: u8,
    ) -> anyhow::Result<()> {
        let key = format!("revoked_vc:{}", vc_hash_hex);
        let value = serde_json::json!({
            "vc_hash": vc_hash_hex,
            "credential_subject_pk": credential_subject_pk,
            "reason": reason,
            "revoked_at": chrono::Utc::now().to_rfc3339(),
        });
        self.db.insert(key, serde_json::to_vec(&value)?)?;
        Ok(())
    }
}
