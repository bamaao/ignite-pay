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

use crate::error::{Result, StateChannelError};
use borsh::{BorshSerialize, BorshDeserialize};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sled::Db;
use solana_program::hash::hash;
use solana_pubkey::Pubkey;
use std::collections::HashMap;

/// Lifecycle state of an HTLC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum HtlcState {
    /// HTLC is active and waiting for preimage reveal or timeout.
    Pending,
    /// Preimage has been revealed; HTLC can be resolved.
    Revealed,
    /// HTLC has been resolved (funds transferred to beneficiary).
    Fulfilled,
    /// HTLC has timed out and been refunded to the original owner.
    Expired,
    /// HTLC has been refunded after timeout (distinct from Fulfilled for audit).
    Refunded,
}

/// Record tracking a single HTLC.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HtlcRecord {
    /// The secret preimage (only stored by the creator/recipient).
    pub preimage: [u8; 32],
    /// SHA-256 hash of the preimage (= hash_lock in the UTXO leaf).
    pub hash_lock: [u8; 32],
    /// Slot when the HTLC was created.
    pub created_slot: u64,
    /// Absolute slot after which the HTLC can be refunded.
    pub timelock_slot: u64,
    /// Amount locked.
    pub amount: u64,
    /// Index of the leaf in the Merkle tree.
    pub leaf_index: usize,
    /// Intended recipient.
    pub beneficiary: Pubkey,
    /// Original owner (for refund).
    pub owner: Pubkey,
    /// Current lifecycle state.
    pub state: HtlcState,
}

/// Manager for HTLC preimages and lifecycle tracking.
pub struct HtlcManager {
    /// Map from hash_lock -> HtlcRecord.
    records: HashMap<[u8; 32], HtlcRecord>,
    /// Optional sled database for persistence.
    db: Option<Db>,
    /// Channel ID used as key prefix for persistence.
    channel_id: Option<[u8; 32]>,
}

impl std::fmt::Debug for HtlcManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HtlcManager")
            .field("num_records", &self.records.len())
            .finish()
    }
}

impl HtlcManager {
    /// Create a new empty HtlcManager (in-memory only).
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            db: None,
            channel_id: None,
        }
    }

    /// Create a new HtlcManager backed by a sled database for persistence.
    pub fn with_db(db: Db, channel_id: [u8; 32]) -> Self {
        let mut mgr = Self {
            records: HashMap::new(),
            db: Some(db),
            channel_id: Some(channel_id),
        };
        mgr.load_from_db().ok();
        mgr
    }

    /// Persist all records to sled using borsh (SER-1 fix: unified with ChannelManager).
    fn persist_to_db(&self) -> Result<()> {
        if let (Some(db), Some(cid)) = (&self.db, &self.channel_id) {
            let key = format!("htlc:{}", hex::encode(cid));
            let records_vec: Vec<_> = self.records.values().cloned().collect();
            let data = borsh::to_vec(&records_vec)
                .map_err(|e| StateChannelError::Other(anyhow::anyhow!("HTLC serialization failed: {}", e)))?;
            db.insert(key.as_bytes(), data)?;
            db.flush()?;
        }
        Ok(())
    }

    /// Load records from sled using borsh.
    fn load_from_db(&mut self) -> Result<()> {
        if let (Some(db), Some(cid)) = (&self.db, &self.channel_id) {
            let key = format!("htlc:{}", hex::encode(cid));
            if let Some(data) = db.get(key.as_bytes())? {
                let records_vec: Vec<HtlcRecord> = borsh::from_slice(&data)
                    .map_err(|e| StateChannelError::Other(anyhow::anyhow!("HTLC deserialization failed: {}", e)))?;
                for record in records_vec {
                    self.records.insert(record.hash_lock, record);
                }
            }
        }
        Ok(())
    }

    /// Create a new HTLC: generate a random preimage, compute hash_lock,
    /// and register the record.
    ///
    /// Returns (hash_lock, preimage).
    pub fn create_htlc(
        &mut self,
        amount: u64,
        leaf_index: usize,
        owner: Pubkey,
        beneficiary: Pubkey,
        current_slot: u64,
        duration_slots: u64,
    ) -> ([u8; 32], [u8; 32]) {
        let mut rng = rand::thread_rng();
        let mut preimage = [0u8; 32];
        rng.fill(&mut preimage);

        let hash_lock = hash(&preimage).to_bytes();
        let timelock_slot = current_slot.saturating_add(duration_slots);

        let record = HtlcRecord {
            preimage,
            hash_lock,
            created_slot: current_slot,
            timelock_slot,
            amount,
            leaf_index,
            beneficiary,
            owner,
            state: HtlcState::Pending,
        };

        self.records.insert(hash_lock, record);
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after create: {}", e);
        }
        (hash_lock, preimage)
    }

    /// Create an HTLC with a known preimage (for testing or cross-party coordination).
    #[allow(clippy::too_many_arguments)]
    pub fn create_htlc_with_preimage(
        &mut self,
        preimage: [u8; 32],
        amount: u64,
        leaf_index: usize,
        owner: Pubkey,
        beneficiary: Pubkey,
        current_slot: u64,
        duration_slots: u64,
    ) -> [u8; 32] {
        let hash_lock = hash(&preimage).to_bytes();
        let timelock_slot = current_slot.saturating_add(duration_slots);

        let record = HtlcRecord {
            preimage,
            hash_lock,
            created_slot: current_slot,
            timelock_slot,
            amount,
            leaf_index,
            beneficiary,
            owner,
            state: HtlcState::Pending,
        };

        self.records.insert(hash_lock, record);
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after create_with_preimage: {}", e);
        }
        hash_lock
    }

    /// Reveal the preimage for a given hash_lock.
    ///
    /// Marks the HTLC as Revealed if the preimage matches.
    pub fn reveal_preimage(&mut self, hash_lock: &[u8; 32], preimage: &[u8; 32]) -> Result<()> {
        let record = self
            .records
            .get_mut(hash_lock)
            .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("HTLC not found")))?;

        if record.state != HtlcState::Pending {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "HTLC is not in pending state"
            )));
        }

        if !Self::verify_preimage(hash_lock, preimage) {
            return Err(StateChannelError::HashLockMismatch);
        }

        record.state = HtlcState::Revealed;
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after reveal: {}", e);
        }
        Ok(())
    }

    /// Get the stored preimage for a hash_lock (if revealed or pending).
    pub fn get_preimage(&self, hash_lock: &[u8; 32]) -> Option<[u8; 32]> {
        self.records.get(hash_lock).map(|r| r.preimage)
    }

    /// Verify that a preimage matches a hash_lock.
    pub fn verify_preimage(hash_lock: &[u8; 32], preimage: &[u8; 32]) -> bool {
        let computed = hash(preimage).to_bytes();
        computed == *hash_lock
    }

    /// Check for expired HTLCs based on the current slot.
    ///
    /// Marks any pending HTLC past its timelock as Expired.
    /// Returns the list of hash_locks that were marked expired.
    pub fn check_expiry(&mut self, current_slot: u64) -> Vec<[u8; 32]> {
        let mut expired = Vec::new();
        for record in self.records.values_mut() {
            if record.state == HtlcState::Pending && current_slot > record.timelock_slot {
                record.state = HtlcState::Expired;
                expired.push(record.hash_lock);
            }
        }
        if !expired.is_empty() {
            if let Err(e) = self.persist_to_db() {
                tracing::warn!("HTLC persistence failed after expiry check: {}", e);
            }
        }
        expired
    }

    /// Mark an HTLC as fulfilled (funds transferred to beneficiary).
    pub fn mark_fulfilled(&mut self, hash_lock: &[u8; 32]) -> Result<()> {
        let record = self
            .records
            .get_mut(hash_lock)
            .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("HTLC not found")))?;

        if record.state != HtlcState::Revealed {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "HTLC must be revealed before fulfilling"
            )));
        }

        record.state = HtlcState::Fulfilled;
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after fulfill: {}", e);
        }
        Ok(())
    }

    /// Mark an HTLC as refunded (funds returned to owner after timeout).
    pub fn mark_refunded(&mut self, hash_lock: &[u8; 32]) -> Result<()> {
        let record = self
            .records
            .get_mut(hash_lock)
            .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("HTLC not found")))?;

        if record.state != HtlcState::Expired {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "HTLC must be expired before refunding"
            )));
        }

        record.state = HtlcState::Refunded; // Distinct from Fulfilled for audit
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after refund: {}", e);
        }
        Ok(())
    }

    /// Get a reference to an HTLC record.
    pub fn get_record(&self, hash_lock: &[u8; 32]) -> Option<&HtlcRecord> {
        self.records.get(hash_lock)
    }

    /// Remove fulfilled, refunded, and expired records.
    pub fn cleanup(&mut self) {
        self.records
            .retain(|_, r| r.state == HtlcState::Pending || r.state == HtlcState::Revealed);
        if let Err(e) = self.persist_to_db() {
            tracing::warn!("HTLC persistence failed after cleanup: {}", e);
        }
    }

    /// Get the number of active (pending or revealed) HTLCs.
    pub fn active_count(&self) -> usize {
        self.records
            .values()
            .filter(|r| r.state == HtlcState::Pending || r.state == HtlcState::Revealed)
            .count()
    }
}

impl Default for HtlcManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_htlc() {
        let mut mgr = HtlcManager::new();
        let owner = Pubkey::new_unique();
        let beneficiary = Pubkey::new_unique();

        let (hash_lock, preimage) = mgr.create_htlc(
            100_000,
            0,
            owner,
            beneficiary,
            100,
            50,
        );

        assert!(HtlcManager::verify_preimage(&hash_lock, &preimage));
        assert_eq!(mgr.active_count(), 1);

        let record = mgr.get_record(&hash_lock).unwrap();
        assert_eq!(record.amount, 100_000);
        assert_eq!(record.timelock_slot, 150);
        assert_eq!(record.state, HtlcState::Pending);
    }

    #[test]
    fn test_reveal_preimage() {
        let mut mgr = HtlcManager::new();
        let (hash_lock, preimage) = mgr.create_htlc(
            100_000, 0, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );

        mgr.reveal_preimage(&hash_lock, &preimage).unwrap();
        let record = mgr.get_record(&hash_lock).unwrap();
        assert_eq!(record.state, HtlcState::Revealed);
    }

    #[test]
    fn test_reveal_wrong_preimage() {
        let mut mgr = HtlcManager::new();
        let (hash_lock, _) = mgr.create_htlc(
            100_000, 0, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );

        let result = mgr.reveal_preimage(&hash_lock, &[99u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fulfill_htlc() {
        let mut mgr = HtlcManager::new();
        let (hash_lock, preimage) = mgr.create_htlc(
            100_000, 0, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );

        mgr.reveal_preimage(&hash_lock, &preimage).unwrap();
        mgr.mark_fulfilled(&hash_lock).unwrap();

        let record = mgr.get_record(&hash_lock).unwrap();
        assert_eq!(record.state, HtlcState::Fulfilled);
    }

    #[test]
    fn test_expiry_and_refund() {
        let mut mgr = HtlcManager::new();
        let (hash_lock, _) = mgr.create_htlc(
            100_000, 0, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );

        // Not expired at slot 120 (timelock = 150)
        let expired = mgr.check_expiry(120);
        assert!(expired.is_empty());

        // Not expired at slot 150 either (strict > per design doc)
        let expired = mgr.check_expiry(150);
        assert!(expired.is_empty());

        // Expired at slot 151 (strictly greater than timelock)
        let expired = mgr.check_expiry(151);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], hash_lock);

        let record = mgr.get_record(&hash_lock).unwrap();
        assert_eq!(record.state, HtlcState::Expired);

        // Refund
        mgr.mark_refunded(&hash_lock).unwrap();
        assert_eq!(mgr.get_record(&hash_lock).unwrap().state, HtlcState::Refunded);
    }

    #[test]
    fn test_cleanup() {
        let mut mgr = HtlcManager::new();

        let (hl1, preimage1) = mgr.create_htlc(
            100_000, 0, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );
        let (hl2, _) = mgr.create_htlc(
            200_000, 1, Pubkey::new_unique(), Pubkey::new_unique(), 100, 50,
        );
        let (hl3, _) = mgr.create_htlc(
            300_000, 2, Pubkey::new_unique(), Pubkey::new_unique(), 100, 200,
        );

        // Fulfill hl1
        mgr.reveal_preimage(&hl1, &preimage1).unwrap();
        mgr.mark_fulfilled(&hl1).unwrap();

        // Expire hl2 (timelock 150, current 200) but not hl3 (timelock 300)
        mgr.check_expiry(200);
        mgr.mark_refunded(&hl2).unwrap();

        // hl3 is still pending (timelock not reached)
        assert_eq!(mgr.active_count(), 1);

        mgr.cleanup();
        assert_eq!(mgr.records.len(), 1);
        assert!(mgr.get_record(&hl1).is_none());
        assert!(mgr.get_record(&hl2).is_none());
        assert!(mgr.get_record(&hl3).is_some());
    }

    #[test]
    fn test_verify_preimage_static() {
        let preimage = [42u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        assert!(HtlcManager::verify_preimage(&hash_lock, &preimage));
        assert!(!HtlcManager::verify_preimage(&hash_lock, &[43u8; 32]));
    }

    #[test]
    fn test_create_htlc_with_preimage() {
        let mut mgr = HtlcManager::new();
        let preimage = [7u8; 32];
        let expected_hash = hash(&preimage).to_bytes();

        let hash_lock = mgr.create_htlc_with_preimage(
            preimage,
            50_000,
            3,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            200,
            100,
        );

        assert_eq!(hash_lock, expected_hash);
        assert!(HtlcManager::verify_preimage(&hash_lock, &preimage));
    }
}
