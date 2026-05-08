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
use crate::merkle::MerkleTree;
use crate::signing::sign_leaf_update;
use crate::types::{LeafUpdate, UTXOLeaf};
use ed25519_dalek::Keypair;
use solana_program::hash::hash as solana_hash;
use solana_pubkey::Pubkey;

/// Pipeline builder for batching multiple leaf updates into a single signed batch.
///
/// The pipeline accumulates operations and produces signed LeafUpdates
/// that can be verified and applied atomically.
///
/// BUG-5 fix: The pipeline stores a backup of the tree leaves on creation.
/// If any operation fails, `abort()` can be called to rollback the tree.
/// If the pipeline is dropped without calling `build()` or `abort()`, the
/// tree is automatically rolled back to its original state (CODE-1 fix).
pub struct Pipeline<'a> {
    tree: &'a mut MerkleTree,
    signer: &'a Keypair,
    channel_id: [u8; 32],
    sequence: u64,
    updates: Vec<LeafUpdate>,
    /// Backup of tree leaves for rollback (BUG-5 fix).
    backup_leaves: Vec<UTXOLeaf>,
    /// Whether build() or abort() has been called (used by Drop).
    consumed: bool,
}

impl<'a> Pipeline<'a> {
    /// Create a new pipeline bound to a mutable tree reference.
    pub fn new(
        tree: &'a mut MerkleTree,
        channel_id: [u8; 32],
        sequence: u64,
        signer: &'a Keypair,
    ) -> Self {
        let backup_leaves = tree.leaves().to_vec();
        Self {
            tree,
            signer,
            channel_id,
            sequence,
            updates: Vec::new(),
            backup_leaves,
            consumed: false,
        }
    }

    /// Get the next sequence number and increment (saturating).
    fn next_sequence(&mut self) -> u64 {
        let seq = self.sequence;
        self.sequence = self.sequence.saturating_add(1);
        seq
    }

    /// Transfer an entire UTXO leaf to a new owner.
    pub fn transfer_leaf(&mut self, index: usize, new_owner: Pubkey) -> Result<()> {
        let prev_leaf = self
            .tree
            .get_leaf(index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        if prev_leaf.is_empty() {
            return Err(StateChannelError::EmptyLeaf);
        }

        let new_leaf = UTXOLeaf::standard(new_owner, prev_leaf.amount);
        let seq = self.next_sequence();

        let update = sign_leaf_update(
            &self.channel_id,
            seq,
            index as u32,
            &prev_leaf,
            new_leaf.clone(),
            self.signer,
        );

        self.tree.update_leaf(index, new_leaf)?;
        self.updates.push(update);
        Ok(())
    }

    /// Partial transfer: split amount from source leaf to a target empty slot.
    ///
    /// BUG-4 fix: Creates dest leaf FIRST (increases total), then deducts source
    /// (decreases total). This preserves the amount conservation invariant at every
    /// intermediate sequence: sum(leaves) >= total_deposited.
    pub fn partial_transfer(
        &mut self,
        src_index: usize,
        dest_index: usize,
        amount: u64,
        recipient: Pubkey,
    ) -> Result<()> {
        let src_leaf = self
            .tree
            .get_leaf(src_index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: src_index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        let dest_leaf = self
            .tree
            .get_leaf(dest_index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: dest_index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        if src_leaf.amount < amount {
            return Err(StateChannelError::InsufficientBalance {
                required: amount,
                available: src_leaf.amount,
            });
        }

        if !dest_leaf.is_empty() {
            return Err(StateChannelError::LeafSlotOccupied);
        }

        // Step 1: Create dest leaf FIRST (increases total — preserves conservation)
        let new_dest = UTXOLeaf::standard(recipient, amount);
        let seq1 = self.next_sequence();
        let update1 = sign_leaf_update(
            &self.channel_id,
            seq1,
            dest_index as u32,
            &dest_leaf,
            new_dest.clone(),
            self.signer,
        );
        self.tree.update_leaf(dest_index, new_dest)?;
        self.updates.push(update1);

        // Step 2: Deduct from source (decreases total — restores conservation)
        let updated_src = UTXOLeaf::standard(src_leaf.owner, src_leaf.amount.saturating_sub(amount));
        let seq2 = self.next_sequence();
        let update2 = sign_leaf_update(
            &self.channel_id,
            seq2,
            src_index as u32,
            &src_leaf,
            updated_src.clone(),
            self.signer,
        );
        self.tree.update_leaf(src_index, updated_src)?;
        self.updates.push(update2);

        Ok(())
    }

    /// Create an HTLC-locked UTXO from an existing standard leaf.
    ///
    /// FLOW-6 fix: Validates that timelock_slot satisfies the design doc §2 constraint:
    /// `timelock_slot > current_slot + CHALLENGE_DURATION + SAFETY_MARGIN`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_htlc(
        &mut self,
        index: usize,
        hash_lock: [u8; 32],
        timelock_slot: u64,
        beneficiary: Pubkey,
        current_slot: u64,
        challenge_duration: u64,
    ) -> Result<()> {
        let prev_leaf = self
            .tree
            .get_leaf(index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        if prev_leaf.is_empty() {
            return Err(StateChannelError::EmptyLeaf);
        }

        // FLOW-6: validate HTLC timelock constraint per design doc §2
        let min_timelock = current_slot
            .saturating_add(challenge_duration)
            .saturating_add(crate::channel::HTLC_SAFETY_MARGIN);
        if timelock_slot <= min_timelock {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "timelock_slot {} must be > current_slot({}) + challenge_duration({}) + safety_margin({}) = {}",
                timelock_slot, current_slot, challenge_duration, crate::channel::HTLC_SAFETY_MARGIN, min_timelock
            )));
        }

        let htlc_leaf = UTXOLeaf::htlc(
            prev_leaf.owner,
            prev_leaf.amount,
            hash_lock,
            timelock_slot,
            beneficiary,
        );

        let seq = self.next_sequence();
        let update = sign_leaf_update(
            &self.channel_id,
            seq,
            index as u32,
            &prev_leaf,
            htlc_leaf.clone(),
            self.signer,
        );

        self.tree.update_leaf(index, htlc_leaf)?;
        self.updates.push(update);
        Ok(())
    }

    /// Resolve an HTLC: replace it with a standard leaf owned by the beneficiary.
    ///
    /// BUG-6 fix: Verifies that the provided preimage matches the hash_lock on the leaf.
    pub fn resolve_htlc(&mut self, index: usize, preimage: &[u8; 32]) -> Result<()> {
        let prev_leaf = self
            .tree
            .get_leaf(index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        if prev_leaf.leaf_type != crate::types::LeafType::HTLC {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf is not an HTLC"
            )));
        }

        // BUG-6 fix: verify preimage matches hash_lock
        let hash_lock = prev_leaf.hash_lock.ok_or_else(|| anyhow::anyhow!("HTLC has no hash_lock"))?;
        let computed_hash = solana_hash(preimage).to_bytes();
        if computed_hash != hash_lock {
            return Err(StateChannelError::HashLockMismatch);
        }

        let resolved = UTXOLeaf::standard(
            prev_leaf.beneficiary.ok_or_else(|| anyhow::anyhow!("HTLC has no beneficiary"))?,
            prev_leaf.amount,
        );
        let seq = self.next_sequence();
        let update = sign_leaf_update(
            &self.channel_id,
            seq,
            index as u32,
            &prev_leaf,
            resolved.clone(),
            self.signer,
        );

        self.tree.update_leaf(index, resolved)?;
        self.updates.push(update);
        Ok(())
    }

    /// Refund an HTLC: replace it with a standard leaf owned by the original owner.
    ///
    /// BUG-6 fix: Verifies that the current slot exceeds the timelock_slot.
    pub fn refund_htlc(&mut self, index: usize, current_slot: u64) -> Result<()> {
        let prev_leaf = self
            .tree
            .get_leaf(index)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.tree.num_leaves(),
            })?
            .clone();

        if prev_leaf.leaf_type != crate::types::LeafType::HTLC {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf is not an HTLC"
            )));
        }

        // BUG-6 fix: verify timelock has expired
        let timelock = prev_leaf.timelock_slot.ok_or_else(|| anyhow::anyhow!("HTLC has no timelock"))?;
        if current_slot <= timelock {
            return Err(StateChannelError::HtlcNotExpired {
                expiry: timelock,
                current: current_slot,
            });
        }

        let refunded = UTXOLeaf::standard(prev_leaf.owner, prev_leaf.amount);
        let seq = self.next_sequence();
        let update = sign_leaf_update(
            &self.channel_id,
            seq,
            index as u32,
            &prev_leaf,
            refunded.clone(),
            self.signer,
        );

        self.tree.update_leaf(index, refunded)?;
        self.updates.push(update);
        Ok(())
    }

    /// Consume the pipeline and return all signed updates plus the final sequence number.
    ///
    /// After calling build(), the pipeline is marked as consumed and will not
    /// rollback the tree on drop.
    pub fn build(mut self) -> (Vec<LeafUpdate>, u64) {
        self.consumed = true;
        let updates = std::mem::take(&mut self.updates);
        let sequence = self.sequence;
        // self will be dropped here, but consumed=true so Drop is a no-op
        (updates, sequence)
    }

    /// Abort the pipeline and rollback the tree to its state before the pipeline was created.
    ///
    /// CODE-5 fix: Provides a way to undo all tree modifications made by pipeline operations.
    pub fn abort(mut self) -> Result<()> {
        let depth = self.tree.tree_depth();
        let restored = MerkleTree::new(std::mem::take(&mut self.backup_leaves), depth)?;
        *self.tree = restored;
        self.consumed = true; // Mark consumed so Drop doesn't try again
        Ok(())
    }
}

impl<'a> Drop for Pipeline<'a> {
    /// CODE-1 fix: Automatically rollback the tree if the pipeline is dropped
    /// without calling build() or abort(). This prevents leaving the tree in a
    /// partially modified state.
    fn drop(&mut self) {
        if !self.consumed {
            let depth = self.tree.tree_depth();
            if let Ok(restored) = MerkleTree::new(std::mem::take(&mut self.backup_leaves), depth) {
                *self.tree = restored;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::ChannelManager;
    use crate::signing::{generate_keypair, to_pubkey};

    fn setup_channel() -> (sled::Db, crate::channel::ChannelState, ed25519_dalek::Keypair, ed25519_dalek::Keypair) {
        let db = tempfile::tempdir().unwrap();
        let db = sled::open(db.path()).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();

        let mgr = ChannelManager::new(db.clone()).unwrap();
        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        (db, state, user, provider)
    }

    #[test]
    fn test_pipeline_transfer() {
        let (db, mut state, user, provider) = setup_channel();

        // Split tree first using the SAME db
        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let recipient = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        {
            let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
            pipeline.transfer_leaf(0, recipient).unwrap();
            let (updates, final_seq) = pipeline.build();
            assert_eq!(updates.len(), 1);
            assert_eq!(final_seq, seq + 2);
            assert_eq!(tree.get_leaf(0).unwrap().owner, recipient);
            assert_eq!(tree.get_leaf(0).unwrap().amount, 500_000);
        }
    }

    #[test]
    fn test_pipeline_partial_transfer() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 800_000),
            UTXOLeaf::standard(to_pubkey(&user), 200_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let recipient = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        {
            let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
            pipeline.partial_transfer(0, 2, 100_000, recipient).unwrap();
            let (updates, _final_seq) = pipeline.build();
            // BUG-4 fix: order is now dest creation + src deduction
            assert_eq!(updates.len(), 2);
            assert_eq!(tree.get_leaf(0).unwrap().amount, 700_000);
            assert_eq!(tree.get_leaf(2).unwrap().amount, 100_000);
            assert_eq!(tree.get_leaf(2).unwrap().owner, recipient);
            assert_eq!(tree.total_amount(), 1_000_000);
        }
    }

    #[test]
    fn test_pipeline_insufficient_balance() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 100_000),
            UTXOLeaf::standard(to_pubkey(&user), 900_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        let result = pipeline.partial_transfer(0, 2, 200_000, Pubkey::new_unique());
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_htlc_create_resolve() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        // Use a real preimage/hash_lock pair so resolve_htlc can verify it
        let preimage = [42u8; 32];
        let hash_lock = solana_program::hash::hash(&preimage).to_bytes();
        let beneficiary = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);

        // Create HTLC
        pipeline.create_htlc(0, hash_lock, 2000, beneficiary, 100, 500).unwrap();

        // Resolve HTLC — BUG-6 fix: must provide correct preimage
        pipeline.resolve_htlc(0, &preimage).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 2); // create + resolve
        assert_eq!(tree.get_leaf(0).unwrap().leaf_type, crate::types::LeafType::Standard);
        assert_eq!(tree.get_leaf(0).unwrap().owner, beneficiary);
        assert_eq!(tree.total_amount(), 1_000_000);
    }

    #[test]
    fn test_pipeline_htlc_resolve_wrong_preimage() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let preimage = [42u8; 32];
        let hash_lock = solana_program::hash::hash(&preimage).to_bytes();
        let beneficiary = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);

        // Create HTLC
        pipeline.create_htlc(0, hash_lock, 2000, beneficiary, 100, 500).unwrap();

        // Try to resolve with wrong preimage — should fail
        let result = pipeline.resolve_htlc(0, &[99u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_htlc_create_refund() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let hash_lock = [42u8; 32];
        let beneficiary = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);

        // Create HTLC with timelock at slot 2000
        pipeline.create_htlc(0, hash_lock, 2000, beneficiary, 100, 500).unwrap();

        // Refund HTLC — BUG-6 fix: must provide current_slot > timelock
        pipeline.refund_htlc(0, 2100).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 2);
        assert_eq!(tree.get_leaf(0).unwrap().leaf_type, crate::types::LeafType::Standard);
        assert_eq!(tree.get_leaf(0).unwrap().owner, to_pubkey(&user));
    }

    #[test]
    fn test_pipeline_htlc_refund_not_expired() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let hash_lock = [42u8; 32];
        let beneficiary = Pubkey::new_unique();
        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);

        // Create HTLC with timelock at slot 2000
        pipeline.create_htlc(0, hash_lock, 2000, beneficiary, 100, 500).unwrap();

        // Try to refund before timelock — should fail (BUG-6 fix)
        let result = pipeline.refund_htlc(0, 400);
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_abort() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let root_before = tree.root();

        {
            let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
            pipeline.transfer_leaf(0, Pubkey::new_unique()).unwrap();
            // Abort instead of build — CODE-5 fix
            pipeline.abort().unwrap();
        }

        // Tree should be rolled back to original state
        assert_eq!(tree.root(), root_before);
        assert_eq!(tree.total_amount(), 1_000_000);
    }

    #[test]
    fn test_pipeline_drop_without_build_aborts() {
        let (db, mut state, user, provider) = setup_channel();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        let mgr = ChannelManager::new(db).unwrap();
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        let channel_id = state.metadata.channel_id;
        let seq = state.metadata.sequence;

        let mut tree = state.tree;
        let root_before = tree.root();

        {
            let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
            pipeline.transfer_leaf(0, Pubkey::new_unique()).unwrap();
            // Drop without build() or abort() — CODE-1 fix: auto-rollback
        }

        // Tree should be rolled back to original state
        assert_eq!(tree.root(), root_before);
        assert_eq!(tree.total_amount(), 1_000_000);
    }
}
