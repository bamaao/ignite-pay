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

use crate::channel::{HOP_MARGIN, min_timelock};
use crate::error::{Result, StateChannelError};
use crate::signing::sign_leaf_update;
use crate::types::{LeafUpdate, UTXOLeaf};
use borsh::{BorshSerialize, BorshDeserialize};
use ed25519_dalek::Keypair;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sled::Db;
use solana_program::hash::hash;
use solana_pubkey::Pubkey;

/// A single hop in a multi-hop payment.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct MultiHopEntry {
    /// Hash lock for this hop (SHA-256 of preimage).
    pub hash_lock: [u8; 32],
    /// Amount for this hop.
    pub amount: u64,
    /// Leaf index in the channel tree.
    pub leaf_index: usize,
    /// Channel ID for this hop.
    pub channel_id: [u8; 32],
    /// Timelock slot for this hop.
    pub timelock_slot: u64,
    /// Owner of the HTLC leaf.
    pub owner: Pubkey,
    /// Beneficiary who can claim with preimage.
    pub beneficiary: Pubkey,
    /// Whether this hop has been resolved.
    pub resolved: bool,
}

/// Status of a multi-hop payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum MultiHopStatus {
    /// Payment is being set up.
    Pending,
    /// All HTLCs are locked on-chain.
    Locked,
    /// Preimage is being propagated backward.
    Resolving,
    /// All hops completed successfully.
    Completed,
    /// Payment failed.
    Failed,
}

/// A multi-hop payment with decreasing timelocks.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct MultiHopPayment {
    /// Unique payment identifier.
    pub payment_id: [u8; 32],
    /// Hash lock shared across all hops.
    pub hash_lock: [u8; 32],
    /// Preimage that unlocks the payment (revealed at the end).
    pub preimage: [u8; 32],
    /// Ordered list of hops.
    pub hops: Vec<MultiHopEntry>,
    /// Current status.
    pub status: MultiHopStatus,
    /// Slot when the payment was created.
    pub created_slot: u64,
}

/// Manager for multi-hop payments backed by sled.
pub struct MultiHopManager {
    db: Db,
}

impl std::fmt::Debug for MultiHopManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiHopManager").finish()
    }
}

impl MultiHopManager {
    /// Create a new MultiHopManager backed by a sled database.
    pub fn new(db: Db) -> Result<Self> {
        Ok(Self { db })
    }

    /// Create a new multi-hop payment with decreasing timelocks.
    ///
    /// Timelock formula: hop[i].timelock = base_timelock - i * HOP_MARGIN
    /// where base_timelock = current_slot + min_timelock(challenge_duration) + (num_hops-1) * HOP_MARGIN
    ///
    /// This ensures each upstream hop has more time than the next downstream hop,
    /// preventing funds from being locked indefinitely.
    pub fn create_payment(
        &self,
        hash_lock: [u8; 32],
        preimage: [u8; 32],
        hops_metadata: Vec<(Pubkey, Pubkey, u64, usize, [u8; 32])>,
        current_slot: u64,
        challenge_duration: u64,
    ) -> Result<MultiHopPayment> {
        let num_hops = hops_metadata.len();
        if num_hops == 0 {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "multi-hop payment requires at least one hop"
            )));
        }

        // Generate unique payment ID
        let mut rng = rand::thread_rng();
        let mut payment_id = [0u8; 32];
        rng.fill(&mut payment_id);

        let min_tl = min_timelock(challenge_duration);
        let base_timelock = current_slot
            .saturating_add(min_tl)
            .saturating_add((num_hops.saturating_sub(1)) as u64 * HOP_MARGIN);

        let mut hops = Vec::with_capacity(num_hops);
        for (i, (owner, beneficiary, amount, leaf_index, channel_id)) in hops_metadata.iter().enumerate() {
            let timelock_slot = base_timelock.saturating_sub((i as u64) * HOP_MARGIN);
            hops.push(MultiHopEntry {
                hash_lock,
                amount: *amount,
                leaf_index: *leaf_index,
                channel_id: *channel_id,
                timelock_slot,
                owner: *owner,
                beneficiary: *beneficiary,
                resolved: false,
            });
        }

        let payment = MultiHopPayment {
            payment_id,
            hash_lock,
            preimage,
            hops,
            status: MultiHopStatus::Pending,
            created_slot: current_slot,
        };

        self.persist_payment(&payment)?;
        Ok(payment)
    }

    /// Create an HTLC leaf update for a specific hop.
    ///
    /// Generates a signed LeafUpdate that creates an HTLC leaf in the channel tree.
    pub fn create_htlc_leaf_update(
        hop: &MultiHopEntry,
        sequence: u64,
        prev_leaf: &UTXOLeaf,
        signer: &Keypair,
    ) -> LeafUpdate {
        let new_leaf = UTXOLeaf::htlc(
            hop.owner,
            hop.amount,
            hop.hash_lock,
            hop.timelock_slot,
            hop.beneficiary,
        );

        sign_leaf_update(
            &hop.channel_id,
            sequence,
            hop.leaf_index as u32,
            prev_leaf,
            new_leaf,
            signer,
        )
    }

    /// Reveal the preimage for a payment and start the resolution phase.
    pub fn reveal_preimage(
        &self,
        payment_id: &[u8; 32],
        preimage: &[u8; 32],
    ) -> Result<MultiHopPayment> {
        let mut payment = self.load_payment(payment_id)?;

        // Verify preimage matches hash_lock
        let computed = hash(preimage).to_bytes();
        if computed != payment.hash_lock {
            return Err(StateChannelError::HashLockMismatch);
        }

        if payment.status != MultiHopStatus::Pending && payment.status != MultiHopStatus::Locked {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "payment is not in a state that allows preimage reveal (status: {:?})",
                payment.status
            )));
        }

        payment.status = MultiHopStatus::Resolving;
        self.persist_payment(&payment)?;
        Ok(payment)
    }

    /// Resolve a specific hop in the payment.
    ///
    /// Marks the hop as resolved. When all hops are resolved, the payment
    /// status transitions to Completed.
    pub fn resolve_hop(
        &self,
        payment_id: &[u8; 32],
        hop_index: usize,
    ) -> Result<MultiHopPayment> {
        let mut payment = self.load_payment(payment_id)?;

        if payment.status != MultiHopStatus::Resolving {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "payment must be in Resolving state to resolve hops"
            )));
        }

        if hop_index >= payment.hops.len() {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "hop index {} out of bounds (max {})",
                hop_index, payment.hops.len()
            )));
        }

        payment.hops[hop_index].resolved = true;

        // Check if all hops are resolved
        if payment.hops.iter().all(|h| h.resolved) {
            payment.status = MultiHopStatus::Completed;
        }

        self.persist_payment(&payment)?;
        Ok(payment)
    }

    /// Check for expired hops in a payment.
    ///
    /// Returns the indices of hops that have expired (current_slot > timelock_slot).
    /// If any hop has expired, marks the payment as Failed.
    pub fn check_expiry(
        &self,
        payment_id: &[u8; 32],
        current_slot: u64,
    ) -> Result<Vec<usize>> {
        let mut payment = self.load_payment(payment_id)?;
        let mut expired = Vec::new();

        for (i, hop) in payment.hops.iter().enumerate() {
            if !hop.resolved && current_slot > hop.timelock_slot {
                expired.push(i);
            }
        }

        if !expired.is_empty() && payment.status != MultiHopStatus::Completed {
            payment.status = MultiHopStatus::Failed;
            self.persist_payment(&payment)?;
        }

        Ok(expired)
    }

    /// Load a payment from the database.
    pub fn load_payment(&self, payment_id: &[u8; 32]) -> Result<MultiHopPayment> {
        let key = format!("multihop:{}", hex::encode(payment_id));
        let data = self
            .db
            .get(key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(hex::encode(payment_id)))?;
        let payment: MultiHopPayment = borsh::from_slice(&data)?;
        Ok(payment)
    }

    /// Persist a payment to the database.
    pub fn persist_payment(&self, payment: &MultiHopPayment) -> Result<()> {
        let key = format!("multihop:{}", hex::encode(payment.payment_id));
        let data = borsh::to_vec(payment)?;
        self.db.insert(key.as_bytes(), data)?;
        self.db.flush()?;
        Ok(())
    }
}

/// Compute per-hop amounts for a multi-hop payment given hub fee rates.
///
/// Per design doc §10.5, routing fees are implicitly implemented by decreasing
/// the amount at each hop. The last hop receives the full `destination_amount`;
/// each upstream hop's amount is `next_hop_amount + fee_for_this_hop`.
///
/// `fee_rates_bps` is ordered from first hop to last hop. Each entry is the
/// fee rate in basis points charged by that hub.
///
/// Returns `Vec<u64>` of amounts for each hop (first to last), or `None` if
/// the fee calculation would overflow or produce insufficient amounts.
pub fn compute_hop_amounts(
    destination_amount: u64,
    fee_rates_bps: &[u16],
) -> Option<Vec<u64>> {
    if fee_rates_bps.is_empty() {
        return None;
    }

    let num_hops = fee_rates_bps.len();
    let mut amounts = vec![0u64; num_hops];

    // Last hop receives the full destination amount
    amounts[num_hops - 1] = destination_amount;

    // Work backwards: each upstream hop adds its fee on top of the downstream amount
    for i in (0..num_hops.saturating_sub(1)).rev() {
        let downstream = amounts[i + 1];
        let fee = downstream
            .checked_mul(fee_rates_bps[i] as u64)?
            .checked_div(10000)?;
        amounts[i] = downstream.checked_add(fee)?;
    }

    Some(amounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{generate_keypair, to_pubkey};
    use crate::types::LeafType;

    const TEST_CHALLENGE_DURATION: u64 = 500;

    fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn make_hop_metadata(count: usize) -> Vec<(Pubkey, Pubkey, u64, usize, [u8; 32])> {
        (0..count)
            .map(|i| {
                (
                    Pubkey::new_unique(),
                    Pubkey::new_unique(),
                    100_000 + i as u64 * 10_000,
                    i,
                    [i as u8; 32],
                )
            })
            .collect()
    }

    #[test]
    fn test_create_payment_basic() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [42u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(3);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();

        assert_eq!(payment.hops.len(), 3);
        assert_eq!(payment.status, MultiHopStatus::Pending);
        assert_eq!(payment.created_slot, 1000);

        // Verify decreasing timelocks
        assert!(payment.hops[0].timelock_slot > payment.hops[1].timelock_slot);
        assert!(payment.hops[1].timelock_slot > payment.hops[2].timelock_slot);

        // Verify timelock spacing
        assert_eq!(
            payment.hops[0].timelock_slot - payment.hops[1].timelock_slot,
            HOP_MARGIN
        );
    }

    #[test]
    fn test_create_payment_single_hop() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [1u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(1);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();
        assert_eq!(payment.hops.len(), 1);
        assert_eq!(payment.hops[0].timelock_slot, 1000 + min_timelock(TEST_CHALLENGE_DURATION));
    }

    #[test]
    fn test_create_payment_no_hops_rejected() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let result = mgr.create_payment([0u8; 32], [0u8; 32], vec![], 1000, TEST_CHALLENGE_DURATION);
        assert!(result.is_err());
    }

    #[test]
    fn test_reveal_preimage() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [7u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(2);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();

        let updated = mgr.reveal_preimage(&payment.payment_id, &preimage).unwrap();
        assert_eq!(updated.status, MultiHopStatus::Resolving);
    }

    #[test]
    fn test_reveal_wrong_preimage_rejected() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [7u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(2);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();

        let result = mgr.reveal_preimage(&payment.payment_id, &[99u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_hop_and_complete() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [5u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(2);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();

        mgr.reveal_preimage(&payment.payment_id, &preimage).unwrap();

        let updated = mgr.resolve_hop(&payment.payment_id, 0).unwrap();
        assert_eq!(updated.status, MultiHopStatus::Resolving);
        assert!(updated.hops[0].resolved);
        assert!(!updated.hops[1].resolved);

        let completed = mgr.resolve_hop(&payment.payment_id, 1).unwrap();
        assert_eq!(completed.status, MultiHopStatus::Completed);
        assert!(completed.hops.iter().all(|h| h.resolved));
    }

    #[test]
    fn test_check_expiry() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [3u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(2);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();

        // Before expiry
        let expired = mgr.check_expiry(&payment.payment_id, 1000).unwrap();
        assert!(expired.is_empty());

        // After last hop's timelock
        let last_timelock = payment.hops.last().unwrap().timelock_slot;
        let expired = mgr.check_expiry(&payment.payment_id, last_timelock + 1).unwrap();
        assert!(!expired.is_empty());

        let loaded = mgr.load_payment(&payment.payment_id).unwrap();
        assert_eq!(loaded.status, MultiHopStatus::Failed);
    }

    #[test]
    fn test_persist_and_load() {
        let db = temp_db();
        let mgr = MultiHopManager::new(db).unwrap();

        let preimage = [8u8; 32];
        let hash_lock = hash(&preimage).to_bytes();
        let hops = make_hop_metadata(2);

        let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, TEST_CHALLENGE_DURATION).unwrap();
        let pid = payment.payment_id;

        let loaded = mgr.load_payment(&pid).unwrap();
        assert_eq!(loaded.payment_id, pid);
        assert_eq!(loaded.hops.len(), 2);
        assert_eq!(loaded.status, MultiHopStatus::Pending);
    }

    #[test]
    fn test_create_htlc_leaf_update() {
        let signer = generate_keypair();
        let hop = MultiHopEntry {
            hash_lock: [1u8; 32],
            amount: 100_000,
            leaf_index: 2,
            channel_id: [5u8; 32],
            timelock_slot: 2000,
            owner: to_pubkey(&signer),
            beneficiary: Pubkey::new_unique(),
            resolved: false,
        };

        let prev_leaf = UTXOLeaf::standard(to_pubkey(&signer), 500_000);
        let update = MultiHopManager::create_htlc_leaf_update(&hop, 1, &prev_leaf, &signer);

        assert_eq!(update.channel_id, [5u8; 32]);
        assert_eq!(update.leaf_index, 2);
        assert_eq!(update.sequence, 1);
        assert_eq!(update.new_leaf.leaf_type, LeafType::HTLC);
        assert_eq!(update.new_leaf.amount, 100_000);
    }

    #[test]
    fn test_compute_hop_amounts_basic() {
        // 3 hops, 10 bps fee each
        let amounts = compute_hop_amounts(1_000_000, &[10, 10, 10]).unwrap();
        assert_eq!(amounts.len(), 3);
        // Last hop gets destination amount
        assert_eq!(amounts[2], 1_000_000);
        // Middle hop adds 10 bps fee on top
        let fee_mid = 1_000_000 * 10 / 10000;
        assert_eq!(amounts[1], 1_000_000 + fee_mid);
        // First hop adds fee on top of middle
        let fee_first = amounts[1] * 10 / 10000;
        assert_eq!(amounts[0], amounts[1] + fee_first);
        // Verify amounts are strictly decreasing from first to last
        assert!(amounts[0] > amounts[1]);
        assert!(amounts[1] > amounts[2]);
    }

    #[test]
    fn test_compute_hop_amounts_single_hop() {
        let amounts = compute_hop_amounts(500_000, &[5]).unwrap();
        assert_eq!(amounts, vec![500_000]);
    }

    #[test]
    fn test_compute_hop_amounts_empty_rejected() {
        assert!(compute_hop_amounts(500_000, &[]).is_none());
    }
}
