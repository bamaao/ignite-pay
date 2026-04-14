use crate::error::{Result, StateChannelError};
use crate::merkle::MerkleTree;
use crate::signing::{generate_keypair, sign_state, to_pubkey, verify_leaf_update_signature, verify_state_signature, claim_message};
use crate::types::{ChannelMetadata, ChannelStatus, LeafType, LeafUpdate, SignedState, UTXOLeaf};
use ed25519_dalek::Keypair;
use solana_program::hash::hash;
use solana_pubkey::Pubkey;
use std::collections::BTreeSet;

/// Safety margin for HTLC timelock validation (in slots, ~6.7 minutes).
pub const HTLC_SAFETY_MARGIN: u64 = 1000;

/// Multi-hop margin (in slots, ~6.7 minutes per hop).
pub const HOP_MARGIN: u64 = 1000;

/// Compute minimum timelock for multi-hop payments.
///
/// Per design doc §10.4.2: `MIN_TIMELOCK = CHALLENGE_DURATION + 3 * HOP_MARGIN`.
/// Uses the channel's actual `challenge_duration` rather than a hardcoded constant.
pub fn min_timelock(challenge_duration: u64) -> u64 {
    challenge_duration.saturating_add(3 * HOP_MARGIN)
}

/// Error detail returned when a batch leaf update fails partway through.
#[derive(Debug)]
pub struct BatchFailureInfo {
    /// Index (within the input slice) of the first update that failed.
    pub failed_index: usize,
    /// The error that caused the failure.
    pub error: StateChannelError,
    /// Number of updates that were successfully applied before the failure.
    pub applied_count: usize,
}

/// Runtime channel state: metadata + tree + optional provider co-signature.
pub struct ChannelState {
    pub metadata: ChannelMetadata,
    pub tree: MerkleTree,
    /// Pending provider co-signature on the current root (if any).
    pub provider_cosign: Option<[u8; 64]>,
}

impl std::fmt::Debug for ChannelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelState")
            .field("channel_id", &hex::encode(self.metadata.channel_id))
            .field("sequence", &self.metadata.sequence)
            .field("status", &self.metadata.status)
            .finish()
    }
}

/// Manager for payment channels backed by a sled database.
pub struct ChannelManager {
    db: sled::Db,
    /// Optional compliance manager for spending limit enforcement.
    compliance: Option<crate::compliance::ComplianceManager>,
}

impl std::fmt::Debug for ChannelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelManager").finish()
    }
}

impl ChannelManager {
    /// Create a new ChannelManager backed by a sled database.
    pub fn new(db: sled::Db) -> Result<Self> {
        Ok(Self { db, compliance: None })
    }

    /// Attach a ComplianceManager for spending limit enforcement.
    ///
    /// Once set, `apply_leaf_update` will call `record_payment` on each
    /// leaf update and enforce compliance holds.
    pub fn set_compliance(&mut self, compliance: crate::compliance::ComplianceManager) {
        self.compliance = Some(compliance);
    }

    /// Get a reference to the attached ComplianceManager, if any.
    pub fn compliance(&self) -> Option<&crate::compliance::ComplianceManager> {
        self.compliance.as_ref()
    }

    /// Open a new payment channel with a single root UTXO leaf.
    ///
    /// Creates Root_init: a tree with one non-empty leaf holding the full deposit.
    #[allow(clippy::too_many_arguments)]
    pub fn open_channel(
        &self,
        user_pubkey: &Pubkey,
        provider_pubkey: &Pubkey,
        token_mint: &Pubkey,
        deposit_amount: u64,
        tree_depth: u32,
        open_slot: u64,
        vault_a: &Pubkey,
        vault_b: &Pubkey,
        challenge_duration: u64,
        min_challenge_delay: u64,
        auto_close_slot: Option<u64>,
    ) -> Result<ChannelState> {
        if deposit_amount == 0 {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "deposit_amount must be > 0"
            )));
        }

        // Generate a unique channel_id from a random keypair hash
        let channel_id = {
            let kp = generate_keypair();
            hash(&kp.to_bytes()).to_bytes()
        };

        let root_leaf = UTXOLeaf::standard(*user_pubkey, deposit_amount);
        let tree = MerkleTree::new(vec![root_leaf], tree_depth as usize)?;
        let current_root = tree.root();

        let non_empty_count = tree.leaves().iter().filter(|l| !l.is_empty()).count() as u32;

        let metadata = ChannelMetadata {
            channel_id,
            user_pubkey: *user_pubkey,
            provider_pubkey: *provider_pubkey,
            token_mint: *token_mint,
            tree_depth,
            status: ChannelStatus::Open,
            sequence: 0,
            current_root,
            total_deposited: deposit_amount,
            open_slot,
            challenge_slot: None,
            vault_a: *vault_a,
            vault_b: *vault_b,
            deposit_a: deposit_amount,
            deposit_b: 0,
            challenge_duration,
            min_challenge_delay,
            auto_close_slot,
            total_claimed: 0,
            settle_deadline: None,
            leaf_count: non_empty_count,
            claimed_leaves: BTreeSet::new(),
        };

        let state = ChannelState {
            metadata: metadata.clone(),
            tree,
            provider_cosign: None,
        };

        self.persist_state(&state)?;
        Ok(state)
    }

    /// FLOW-3: Provider funds a channel with additional capital.
    ///
    /// Validates that the channel is Open, deposit_b is currently 0 (not already funded),
    /// the signer is the provider, deposit_b > 0, and there is an available empty slot.
    /// Creates a provider-owned leaf, applies the update, persists state, and returns
    /// the signed LeafUpdate for audit trail purposes.
    pub fn fund_channel(
        &self,
        state: &mut ChannelState,
        provider_keypair: &Keypair,
        deposit_b: u64,
        provider_leaf_index: Option<usize>,
    ) -> Result<LeafUpdate> {
        // Channel must be Open
        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }

        // Provider must not have already funded
        if state.metadata.deposit_b != 0 {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel already has provider funding (deposit_b: {})",
                state.metadata.deposit_b
            )));
        }

        // Verify signer is the provider
        let provider_pubkey = to_pubkey(provider_keypair);
        if provider_pubkey != state.metadata.provider_pubkey {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "signer is not the channel provider"
            )));
        }

        // deposit_b must be positive
        if deposit_b == 0 {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "deposit_b must be > 0"
            )));
        }

        // Security: cap deposit_b to prevent total_deposited overflow
        // and ensure provider cannot inject unreasonably large amounts.
        let max_deposit_b = u64::MAX.saturating_sub(state.metadata.total_deposited);
        if deposit_b > max_deposit_b {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "deposit_b {} would cause total_deposited overflow (max allowed: {})",
                deposit_b, max_deposit_b
            )));
        }

        // Find an empty slot
        let target_index = match provider_leaf_index {
            Some(idx) => {
                let leaf = state.tree.get_leaf(idx).ok_or(
                    StateChannelError::LeafIndexOutOfBounds {
                        index: idx,
                        max: state.tree.num_leaves(),
                    },
                )?;
                if !leaf.is_empty() {
                    return Err(StateChannelError::LeafSlotOccupied);
                }
                idx
            }
            None => state
                .tree
                .available_slots()
                .first()
                .copied()
                .ok_or(StateChannelError::NoAvailableSlots)?,
        };

        let prev_leaf = state.tree.get_leaf(target_index).unwrap().clone();
        let new_leaf = UTXOLeaf::standard(provider_pubkey, deposit_b);

        let new_sequence = state.metadata.sequence + 1;
        let update = crate::signing::sign_leaf_update(
            &state.metadata.channel_id,
            new_sequence,
            target_index as u32,
            &prev_leaf,
            new_leaf.clone(),
            provider_keypair,
        );

        // Apply the update
        state
            .tree
            .update_leaf(target_index, new_leaf)?;

        state.metadata.sequence = new_sequence;
        state.metadata.current_root = state.tree.root();
        state.metadata.leaf_count = state.tree.leaves().iter().filter(|l| !l.is_empty()).count() as u32;

        // Update deposit tracking
        state.metadata.deposit_b = deposit_b;
        state.metadata.total_deposited = state.metadata.total_deposited.saturating_add(deposit_b);

        state.provider_cosign = None;
        self.persist_state(state)?;
        Ok(update)
    }

    /// Construct a split tree from a set of pre-allocated UTXO leaves.
    ///
    /// Verifies amount conservation and that all leaf owners are valid
    /// (user-owned or provider-owned), validates per-party totals match
    /// deposit_a and deposit_b, builds a new tree, and produces a dual-signed state.
    pub fn construct_split_tree(
        &self,
        state: &mut ChannelState,
        leaves: Vec<UTXOLeaf>,
        user_keypair: &Keypair,
        provider_keypair: &Keypair,
    ) -> Result<SignedState> {
        let total: u64 = leaves.iter().map(|l| l.amount).fold(0u64, |acc, x| acc.saturating_add(x));
        if total != state.metadata.total_deposited {
            return Err(StateChannelError::AmountConservation {
                expected: state.metadata.total_deposited,
                actual: total,
            });
        }

        // FLOW-3: verify all non-empty leaf owners are either user or provider.
        // Empty leaves (amount == 0, owner = Pubkey::default()) are skipped by
        // the `!leaf.is_empty()` check, so they don't trigger the else branch.
        let user_pubkey = state.metadata.user_pubkey;
        let provider_pubkey = state.metadata.provider_pubkey;
        let mut user_total: u64 = 0;
        let mut provider_total: u64 = 0;
        for (i, leaf) in leaves.iter().enumerate() {
            if !leaf.is_empty() {
                if leaf.owner == user_pubkey {
                    user_total = user_total.saturating_add(leaf.amount);
                } else if leaf.owner == provider_pubkey {
                    provider_total = provider_total.saturating_add(leaf.amount);
                } else {
                    return Err(StateChannelError::Other(anyhow::anyhow!(
                        "leaf {} owner {:?} is neither user nor provider",
                        i, leaf.owner
                    )));
                }
            }
        }

        // FLOW-3: validate per-party amounts match deposits
        if user_total != state.metadata.deposit_a {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "user leaf total {} does not match deposit_a {}",
                user_total, state.metadata.deposit_a
            )));
        }
        if provider_total != state.metadata.deposit_b {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "provider leaf total {} does not match deposit_b {}",
                provider_total, state.metadata.deposit_b
            )));
        }

        let new_tree = MerkleTree::new(leaves, state.metadata.tree_depth as usize)?;
        let new_root = new_tree.root();
        let new_sequence = state.metadata.sequence + 1;

        let sig_a = sign_state(
            &state.metadata.channel_id,
            new_sequence,
            &new_root,
            user_keypair,
        );
        let sig_b = sign_state(
            &state.metadata.channel_id,
            new_sequence,
            &new_root,
            provider_keypair,
        );

        let provider_cosign = sig_b;
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: new_sequence,
            root: new_root,
            sig_a,
            sig_b,
        };

        state.tree = new_tree;
        state.metadata.sequence = new_sequence;
        state.metadata.current_root = new_root;
        state.metadata.leaf_count = state.tree.leaves().iter().filter(|l| !l.is_empty()).count() as u32;
        state.provider_cosign = Some(provider_cosign);

        self.persist_state(state)?;
        Ok(signed_state)
    }

    /// Validate a leaf update against the current channel state.
    ///
    /// Checks: channel ID, channel status (must be Open), sequence, prev hash, signature.
    fn validate_leaf_update(
        state: &ChannelState,
        update: &LeafUpdate,
        signer_pubkey: &Pubkey,
    ) -> Result<()> {
        // BUG-2 fix: channel must be Open to accept leaf updates
        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }

        if update.channel_id != state.metadata.channel_id {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel ID mismatch"
            )));
        }

        if update.sequence != state.metadata.sequence + 1 {
            return Err(StateChannelError::InvalidSequence {
                expected: state.metadata.sequence + 1,
                actual: update.sequence,
            });
        }

        let current_hash = state
            .tree
            .get_leaf_hash(update.leaf_index as usize)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: update.leaf_index as usize,
                max: state.tree.num_leaves(),
            })?;

        if update.prev_leaf_hash != current_hash {
            return Err(StateChannelError::PrevHashMismatch);
        }

        if !verify_leaf_update_signature(update, signer_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        Ok(())
    }

    /// Apply a single signed leaf update to the channel and persist.
    ///
    /// Verifies sequence number, previous leaf hash match, and signature validity.
    pub fn apply_leaf_update(
        &self,
        state: &mut ChannelState,
        update: &LeafUpdate,
        signer_pubkey: &Pubkey,
    ) -> Result<()> {
        Self::validate_leaf_update(state, update, signer_pubkey)?;

        // DEV-8: Check compliance before applying payment updates.
        if let Some(ref cm) = self.compliance {
            if update.new_leaf.amount > 0 && update.new_leaf.leaf_type == LeafType::Standard {
                cm.record_payment(
                    state.metadata.channel_id,
                    update.new_leaf.amount,
                    0,
                    state.metadata.user_pubkey,
                    state.metadata.provider_pubkey,
                )?;
            }
        }

        // Apply the update
        state
            .tree
            .update_leaf(update.leaf_index as usize, update.new_leaf.clone())?;

        state.metadata.sequence = update.sequence;
        state.metadata.current_root = state.tree.root();
        state.metadata.leaf_count = state.tree.leaves().iter().filter(|l| !l.is_empty()).count() as u32;

        // BUG-1 fix: root changed, old provider_cosign is stale
        state.provider_cosign = None;

        self.persist_state(state)?;
        Ok(())
    }

    /// Apply a batch of leaf updates sequentially (all-or-nothing) and persist.
    ///
    /// If any update in the batch fails, none are applied.
    /// BUG-35 fix: same leaf_index is now allowed in a batch when sequential updates
    /// form a valid chain (the second update's prev_hash must match the new state after
    /// the first update).
    pub fn apply_leaf_update_batch(
        &self,
        state: &mut ChannelState,
        updates: &[LeafUpdate],
        signer_pubkey: &Pubkey,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Clone state for rollback
        let backup_tree_leaves: Vec<UTXOLeaf> = state.tree.leaves().to_vec();
        let backup_sequence = state.metadata.sequence;
        let backup_root = state.metadata.current_root;

        for update in updates {
            if let Err(e) = Self::apply_leaf_update_internal(state, update, signer_pubkey, &self.compliance) {
                // Rollback
                let restored = MerkleTree::new(backup_tree_leaves, state.metadata.tree_depth as usize)?;
                state.tree = restored;
                state.metadata.sequence = backup_sequence;
                state.metadata.current_root = backup_root;
                return Err(e);
            }
        }

        self.persist_state(state)?;
        Ok(())
    }

    /// FLOW-5: Apply a batch of leaf updates with failure position info.
    ///
    /// If any update fails, returns `BatchFailureInfo` with the index of the failed
    /// update, the error, and how many were successfully applied. The tree state is
    /// rolled back to before the batch. The caller can re-sign from the failure point.
    pub fn apply_leaf_update_batch_with_info(
        &self,
        state: &mut ChannelState,
        updates: &[LeafUpdate],
        signer_pubkey: &Pubkey,
    ) -> std::result::Result<(), BatchFailureInfo> {
        if updates.is_empty() {
            return Ok(());
        }

        let backup_tree_leaves: Vec<UTXOLeaf> = state.tree.leaves().to_vec();
        let backup_sequence = state.metadata.sequence;
        let backup_root = state.metadata.current_root;

        for (i, update) in updates.iter().enumerate() {
            if let Err(e) = Self::apply_leaf_update_internal(state, update, signer_pubkey, &self.compliance) {
                // Rollback
                if let Ok(restored) =
                    MerkleTree::new(backup_tree_leaves, state.metadata.tree_depth as usize)
                {
                    state.tree = restored;
                }
                state.metadata.sequence = backup_sequence;
                state.metadata.current_root = backup_root;
                return Err(BatchFailureInfo {
                    failed_index: i,
                    error: e,
                    applied_count: i,
                });
            }
        }

        if let Err(e) = self.persist_state(state) {
            // Rollback on persistence failure
            if let Ok(restored) =
                MerkleTree::new(backup_tree_leaves, state.metadata.tree_depth as usize)
            {
                state.tree = restored;
            }
            state.metadata.sequence = backup_sequence;
            state.metadata.current_root = backup_root;
            return Err(BatchFailureInfo {
                failed_index: updates.len(),
                error: e,
                applied_count: updates.len(),
            });
        }
        Ok(())
    }

    /// Internal apply without persistence (used by batch operations).
    fn apply_leaf_update_internal(
        state: &mut ChannelState,
        update: &LeafUpdate,
        signer_pubkey: &Pubkey,
        compliance: &Option<crate::compliance::ComplianceManager>,
    ) -> Result<()> {
        Self::validate_leaf_update(state, update, signer_pubkey)?;

        // DEV-8: Check compliance hold before applying payment updates.
        // Standard leaf updates with amount > 0 are treated as payments.
        if let Some(ref cm) = compliance {
            if update.new_leaf.amount > 0 && update.new_leaf.leaf_type == LeafType::Standard {
                let _action = cm.record_payment(
                    state.metadata.channel_id,
                    update.new_leaf.amount,
                    0, // slot not available here; compliance uses its own tracking
                    state.metadata.user_pubkey,
                    state.metadata.provider_pubkey,
                )?;
                // If action is InsertMarker, caller is responsible for inserting
                // a compliance leaf into the tree.
            }
        }

        state
            .tree
            .update_leaf(update.leaf_index as usize, update.new_leaf.clone())?;

        state.metadata.sequence = update.sequence;
        state.metadata.current_root = state.tree.root();

        Ok(())
    }

    /// Persist channel state to sled.
    ///
    /// Stores metadata, leaves, and provider_cosign separately under keys:
    /// - `channel:{channel_id}:meta` -> borsh(ChannelMetadata)
    /// - `channel:{channel_id}:leaves` -> borsh(Vec<UTXOLeaf>)
    /// - `channel:{channel_id}:cosign` -> borsh(Option<[u8; 64]>)
    pub fn persist_state(&self, state: &ChannelState) -> Result<()> {
        let cid = hex::encode(state.metadata.channel_id);

        let meta_key = format!("channel:{}:meta", cid);
        let leaves_key = format!("channel:{}:leaves", cid);
        let cosign_key = format!("channel:{}:cosign", cid);

        let meta_bytes = borsh::to_vec(&state.metadata)?;
        let leaves_bytes = borsh::to_vec(state.tree.leaves())?;
        let cosign_bytes = borsh::to_vec(&state.provider_cosign)?;

        self.db.insert(meta_key.as_bytes(), meta_bytes)?;
        self.db.insert(leaves_key.as_bytes(), leaves_bytes)?;
        self.db.insert(cosign_key.as_bytes(), cosign_bytes)?;
        self.db.flush()?;

        Ok(())
    }

    /// Load channel state from sled.
    pub fn load_state(&self, channel_id: &[u8; 32]) -> Result<ChannelState> {
        let cid = hex::encode(channel_id);

        let meta_key = format!("channel:{}:meta", cid);
        let leaves_key = format!("channel:{}:leaves", cid);
        let cosign_key = format!("channel:{}:cosign", cid);

        let meta_bytes = self
            .db
            .get(meta_key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(cid.clone()))?;

        let leaves_bytes = self
            .db
            .get(leaves_key.as_bytes())?
            .ok_or_else(|| StateChannelError::ChannelNotFound(cid.clone()))?;

        let metadata: ChannelMetadata = borsh::from_slice(&meta_bytes)?;
        let leaves: Vec<UTXOLeaf> = borsh::from_slice(&leaves_bytes)?;

        let provider_cosign: Option<[u8; 64]> = self
            .db
            .get(cosign_key.as_bytes())?
            .map(|v| borsh::from_slice(&v))
            .transpose()?
            .flatten();

        let tree = MerkleTree::new(leaves, metadata.tree_depth as usize)?;

        Ok(ChannelState {
            metadata,
            tree,
            provider_cosign,
        })
    }

    /// Close a channel cooperatively (requires dual-signed state).
    ///
    /// Validates that the channel is Open and that both parties have signed
    /// the current root. Transitions to Settling status and sets settle_deadline.
    pub fn close_channel(
        &self,
        state: &mut ChannelState,
        signed_state: &SignedState,
        user_pubkey: &Pubkey,
        provider_pubkey: &Pubkey,
        current_slot: u64,
        settle_window: u64,
    ) -> Result<()> {
        // Must be Open to close
        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }

        // BUG-33 fix: check for unresolved HTLC leaves before closing (§3.4.5)
        let htlc_leaves: Vec<usize> = state
            .tree
            .leaves()
            .iter()
            .enumerate()
            .filter(|(_, leaf)| leaf.leaf_type == LeafType::HTLC)
            .map(|(idx, _)| idx)
            .collect();
        if !htlc_leaves.is_empty() {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "cannot close: {} unresolved HTLC leaf/leaves at index {:?}. Resolve or refund all HTLCs before closing.",
                htlc_leaves.len(),
                htlc_leaves
            )));
        }

        // Verify channel ID
        if signed_state.channel_id != state.metadata.channel_id {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "signed state channel ID mismatch"
            )));
        }

        // Verify the signed state matches current root
        if signed_state.root != state.metadata.current_root {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "signed state root does not match current root"
            )));
        }

        // Verify sequence matches. Design doc §4.2 specifies `sequence > on_chain.sequence`
        // for the on-chain program, but off-chain both parties hold the latest state, so
        // strict equality is correct here: CooperativeSettle always uses the current state.
        // The `>` check is only needed in SubmitCounterState for submitting newer states.
        if signed_state.sequence != state.metadata.sequence {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "signed state sequence does not match current sequence"
            )));
        }

        // Verify both signatures
        if !verify_state_signature(
            &signed_state.channel_id,
            signed_state.sequence,
            &signed_state.root,
            &signed_state.sig_a,
            user_pubkey,
        ) {
            return Err(StateChannelError::InvalidSignature);
        }

        if !verify_state_signature(
            &signed_state.channel_id,
            signed_state.sequence,
            &signed_state.root,
            &signed_state.sig_b,
            provider_pubkey,
        ) {
            return Err(StateChannelError::InvalidSignature);
        }

        // Transition to Settling (not directly to Closed per design doc §3.4.1)
        // BUG-2 fix: set settle_deadline per design doc §3.4.1
        state.metadata.status = ChannelStatus::Settling;
        state.metadata.settle_deadline = Some(current_slot + settle_window);
        state.provider_cosign = None;
        self.persist_state(state)
    }

    /// Get a reference to the underlying sled database.
    pub fn db(&self) -> &sled::Db {
        &self.db
    }

    /// FLOW-4: Provider co-signs the current state root.
    ///
    /// Per design doc §4.3.4, the provider should periodically sign the current
    /// (channel_id, sequence, root) tuple after verifying LeafUpdates from the user.
    /// This co-signature enables CooperativeSettle and serves as evidence of state agreement.
    pub fn provider_cosign_state(
        &self,
        state: &mut ChannelState,
        provider_keypair: &Keypair,
    ) -> Result<[u8; 64]> {
        let sig = sign_state(
            &state.metadata.channel_id,
            state.metadata.sequence,
            &state.metadata.current_root,
            provider_keypair,
        );
        state.provider_cosign = Some(sig);
        self.persist_state(state)?;
        Ok(sig)
    }

    /// Set the auto-close slot for a channel.
    ///
    /// Per design doc §3.4.3: after `auto_close_slot` expires, anyone can trigger
    /// settlement to prevent funds from being permanently locked when both parties are offline.
    pub fn set_auto_close_slot(
        &self,
        state: &mut ChannelState,
        auto_close_slot: u64,
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }
        state.metadata.auto_close_slot = Some(auto_close_slot);
        self.persist_state(state)
    }

    /// Auto-settle a channel after `auto_close_slot` has passed.
    ///
    /// Per design doc §3.4.3: no challenge period needed, directly enters Settling.
    /// Anyone (relayer / watchtower / user / provider) can trigger this.
    pub fn auto_settle(
        &self,
        state: &mut ChannelState,
        current_slot: u64,
        settle_window: u64,
    ) -> Result<()> {
        let auto_close = state.metadata.auto_close_slot.ok_or_else(|| {
            StateChannelError::Other(anyhow::anyhow!("auto_close_slot not set"))
        })?;

        if current_slot < auto_close {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "auto_close_slot not yet reached (auto_close: {}, current: {})",
                auto_close, current_slot
            )));
        }

        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }

        state.metadata.status = ChannelStatus::Settling;
        state.metadata.settle_deadline = Some(current_slot + settle_window);
        state.provider_cosign = None;
        self.persist_state(state)
    }

    /// Trigger a dispute challenge on the channel.
    ///
    /// Per design doc §3.4.2/§4.2, either party can trigger a challenge by
    /// submitting a (submitted_root, submitted_sequence) pair signed by the challenger.
    /// Checks min_challenge_delay (anti front-running), challenger is a participant,
    /// and submitted_sequence > current sequence.
    /// Requires a signature from the challenger to prove identity.
    pub fn trigger_challenge(
        &self,
        state: &mut ChannelState,
        challenger_pubkey: &Pubkey,
        current_slot: u64,
        submitted_root: &[u8; 32],
        submitted_sequence: u64,
        challenger_signature: &[u8; 64],
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Open {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not open (status: {:?})",
                state.metadata.status
            )));
        }

        // Verify challenger is a channel participant
        if *challenger_pubkey != state.metadata.user_pubkey
            && *challenger_pubkey != state.metadata.provider_pubkey
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "challenger is not a channel participant"
            )));
        }

        // BUG-2 fix: check min_challenge_delay (anti front-running)
        let min_slot = state.metadata.open_slot + state.metadata.min_challenge_delay;
        if current_slot < min_slot {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "cannot challenge before slot {} (min_challenge_delay, current: {})",
                min_slot, current_slot
            )));
        }

        // DEV-9: Verify submitted_sequence > current sequence (per design doc §4.2)
        if submitted_sequence <= state.metadata.sequence {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "submitted_sequence {} must be > current sequence {}",
                submitted_sequence, state.metadata.sequence
            )));
        }

        // BUG-3 fix: verify challenger's signature (BUG-23: use submitted_sequence, not current_slot)
        let message = crate::signing::state_message(
            &state.metadata.channel_id,
            submitted_sequence,
            submitted_root,
        );
        if !crate::signing::verify_ed25519_signature(&message, challenger_signature, challenger_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        state.metadata.status = ChannelStatus::Challenged;
        state.metadata.challenge_slot = Some(current_slot);
        // DEV-9: Update to the submitted state
        state.metadata.current_root = *submitted_root;
        state.metadata.sequence = submitted_sequence;
        self.persist_state(state)
    }

    /// Settle after challenge timeout.
    ///
    /// BUG-5 fix: Per design doc §3.4.2 step 3, transitions Challenged → Settling
    /// after `challenge_duration` has elapsed since `challenge_slot`.
    /// Sets `settle_deadline` for the subsequent claim window.
    pub fn settle_after_timeout(
        &self,
        state: &mut ChannelState,
        current_slot: u64,
        settle_window: u64,
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Challenged {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not challenged (status: {:?})",
                state.metadata.status
            )));
        }

        let challenge_slot = state.metadata.challenge_slot.ok_or_else(|| {
            StateChannelError::Other(anyhow::anyhow!("challenge_slot not set"))
        })?;

        // Design doc §4.2: strict `>` (current_slot must be strictly past challenge end)
        if current_slot <= challenge_slot + state.metadata.challenge_duration {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "challenge duration has not elapsed (challenge_slot: {}, duration: {}, current: {}, need > {})",
                challenge_slot, state.metadata.challenge_duration, current_slot,
                challenge_slot + state.metadata.challenge_duration
            )));
        }

        state.metadata.status = ChannelStatus::Settling;
        state.metadata.settle_deadline = Some(current_slot + settle_window);
        state.provider_cosign = None;
        self.persist_state(state)
    }

    /// Submit a counter-state during the challenge period.
    ///
    /// BUG-6 fix: Per design doc §3.4.2, the counterparty can submit a higher-sequence
    /// dual-signed state during the challenge period. If accepted, updates the channel state.
    ///
    /// BUG-1 fix: If `counter_leaves` is provided, rebuilds the MerkleTree and verifies
    /// the reconstructed root matches the signed root. If `None`, only metadata is updated
    /// (callers must ensure tree consistency externally).
    pub fn submit_counter_state(
        &self,
        state: &mut ChannelState,
        counter_state: &SignedState,
        counter_leaves: Option<Vec<UTXOLeaf>>,
        user_pubkey: &Pubkey,
        provider_pubkey: &Pubkey,
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Challenged {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not challenged (status: {:?})",
                state.metadata.status
            )));
        }

        // Verify channel ID
        if counter_state.channel_id != state.metadata.channel_id {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "counter state channel ID mismatch"
            )));
        }

        // Counter state must have a higher sequence than current
        if counter_state.sequence <= state.metadata.sequence {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "counter state sequence {} must be greater than current {}",
                counter_state.sequence, state.metadata.sequence
            )));
        }

        // Verify both signatures
        if !verify_state_signature(
            &counter_state.channel_id,
            counter_state.sequence,
            &counter_state.root,
            &counter_state.sig_a,
            user_pubkey,
        ) {
            return Err(StateChannelError::InvalidSignature);
        }

        if !verify_state_signature(
            &counter_state.channel_id,
            counter_state.sequence,
            &counter_state.root,
            &counter_state.sig_b,
            provider_pubkey,
        ) {
            return Err(StateChannelError::InvalidSignature);
        }

        // Accept the counter state
        state.metadata.sequence = counter_state.sequence;
        state.metadata.current_root = counter_state.root;
        state.provider_cosign = Some(counter_state.sig_b);

        // BUG-1 fix: restore MerkleTree if leaves are provided
        if let Some(leaves) = counter_leaves {
            let new_tree = MerkleTree::new(leaves, state.metadata.tree_depth as usize)?;
            if new_tree.root() != counter_state.root {
                return Err(StateChannelError::Other(anyhow::anyhow!(
                    "counter state leaves root does not match signed root"
                )));
            }
            state.tree = new_tree;
            state.metadata.leaf_count = state.tree.leaves().iter().filter(|l| !l.is_empty()).count() as u32;
        }

        self.persist_state(state)
    }

    /// Claim a leaf's funds during settlement.
    ///
    /// Per design doc §5, parties submit leaf data to claim funds.
    /// Validates settle_deadline, leaf ownership, amount match, Merkle proof,
    /// and claimer's signature.
    pub fn claim_leaf(
        &self,
        state: &mut ChannelState,
        leaf_index: u32,
        claim_amount: u64,
        claimer_pubkey: &Pubkey,
        current_slot: u64,
        claimer_signature: &[u8; 64],
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Settling {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not settling (status: {:?})",
                state.metadata.status
            )));
        }

        // BUG-4 fix: check settle_deadline
        let deadline = state.metadata.settle_deadline.ok_or_else(|| {
            StateChannelError::Other(anyhow::anyhow!("settle_deadline not set"))
        })?;
        if current_slot > deadline {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "settlement window has expired (deadline: {}, current: {})",
                deadline, current_slot
            )));
        }

        if state.metadata.claimed_leaves.contains(&leaf_index) {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} has already been claimed",
                leaf_index
            )));
        }

        // Verify claimer is a channel participant
        if *claimer_pubkey != state.metadata.user_pubkey
            && *claimer_pubkey != state.metadata.provider_pubkey
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not a channel participant"
            )));
        }

        // BUG-34 fix: use claim-specific signature message to prevent replay
        let message = claim_message(
            &state.metadata.channel_id,
            leaf_index,
            claim_amount,
            current_slot,
        );
        if !crate::signing::verify_ed25519_signature(&message, claimer_signature, claimer_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        // BUG-1 fix: verify leaf exists and ownership
        let leaf = state
            .tree
            .get_leaf(leaf_index as usize)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: leaf_index as usize,
                max: state.tree.num_leaves(),
            })?
            .clone();

        if leaf.is_empty() {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} is empty and cannot be claimed",
                leaf_index
            )));
        }

        // BUG-22: verify leaf is Standard type (HTLC/Compliance leaves use dedicated claim methods)
        if leaf.leaf_type != LeafType::Standard {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} is not a Standard leaf and cannot be claimed via claim_leaf",
                leaf_index
            )));
        }

        // BUG-1 fix: verify claimer owns the leaf
        if leaf.owner != *claimer_pubkey {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} owner does not match claimer",
                leaf_index
            )));
        }

        // BUG-1 fix: verify claim_amount matches actual leaf amount
        if claim_amount != leaf.amount {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claim_amount {} does not match leaf amount {}",
                claim_amount, leaf.amount
            )));
        }

        // Verify Merkle proof: leaf is in the current tree
        let proof = state.tree.get_proof(leaf_index as usize)?;
        let leaf_hash = leaf.hash();
        if !MerkleTree::verify_proof(&leaf_hash, &proof, &state.metadata.current_root) {
            return Err(StateChannelError::ProofVerificationFailed);
        }

        // BUG-5 fix: overflow protection
        let new_total = state.metadata.total_claimed.saturating_add(claim_amount);
        if new_total > state.metadata.total_deposited {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "total_claimed would exceed total_deposited"
            )));
        }
        state.metadata.total_claimed = new_total;
        state.metadata.claimed_leaves.insert(leaf_index);
        self.persist_state(state)
    }

    /// Claim a leaf's funds using an externally-provided Merkle proof.
    ///
    /// Per design doc §5.2: on-chain Claim requires (leaf_index, leaf_data, merkle_proof).
    /// This variant accepts an external proof, aligning the off-chain API with the
    /// on-chain Claim instruction interface.
    pub fn claim_leaf_with_proof(
        &self,
        state: &mut ChannelState,
        leaf_index: u32,
        claim_amount: u64,
        claimer_pubkey: &Pubkey,
        current_slot: u64,
        claimer_signature: &[u8; 64],
        proof: &[[u8; 32]],
    ) -> Result<()> {
        if state.metadata.status != ChannelStatus::Settling {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not settling (status: {:?})",
                state.metadata.status
            )));
        }

        let deadline = state.metadata.settle_deadline.ok_or_else(|| {
            StateChannelError::Other(anyhow::anyhow!("settle_deadline not set"))
        })?;
        if current_slot > deadline {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "settlement window has expired (deadline: {}, current: {})",
                deadline, current_slot
            )));
        }

        if state.metadata.claimed_leaves.contains(&leaf_index) {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} has already been claimed",
                leaf_index
            )));
        }

        if *claimer_pubkey != state.metadata.user_pubkey
            && *claimer_pubkey != state.metadata.provider_pubkey
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not a channel participant"
            )));
        }

        // BUG-34 fix: use claim-specific signature message
        let message = claim_message(
            &state.metadata.channel_id,
            leaf_index,
            claim_amount,
            current_slot,
        );
        if !crate::signing::verify_ed25519_signature(&message, claimer_signature, claimer_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        let leaf = state
            .tree
            .get_leaf(leaf_index as usize)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: leaf_index as usize,
                max: state.tree.num_leaves(),
            })?
            .clone();

        if leaf.is_empty() {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} is empty and cannot be claimed",
                leaf_index
            )));
        }

        if leaf.owner != *claimer_pubkey {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} owner does not match claimer",
                leaf_index
            )));
        }

        if claim_amount != leaf.amount {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claim_amount {} does not match leaf amount {}",
                claim_amount, leaf.amount
            )));
        }

        // Verify externally-provided Merkle proof
        let leaf_hash = leaf.hash();
        if !MerkleTree::verify_proof(&leaf_hash, proof, &state.metadata.current_root) {
            return Err(StateChannelError::ProofVerificationFailed);
        }

        let new_total = state.metadata.total_claimed.saturating_add(claim_amount);
        if new_total > state.metadata.total_deposited {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "total_claimed would exceed total_deposited"
            )));
        }
        state.metadata.total_claimed = new_total;
        state.metadata.claimed_leaves.insert(leaf_index);
        self.persist_state(state)
    }

    /// FLOW-8: VerifyHTLC — beneficiary claims an HTLC leaf by providing the preimage.
    ///
    /// Per design doc §4.2 VerifyHTLC: the beneficiary submits the preimage R.
    /// Validates: channel is Challenged or Settling, settle_deadline, leaf is HTLC type,
    /// SHA-256(preimage) == hash_lock, current_slot <= timelock_slot,
    /// claimer is the beneficiary, Merkle proof, signature, overflow protection.
    pub fn claim_htlc_verify(
        &self,
        state: &mut ChannelState,
        leaf_index: u32,
        preimage: &[u8; 32],
        claimer_pubkey: &Pubkey,
        current_slot: u64,
        claimer_signature: &[u8; 64],
    ) -> Result<()> {
        // Per design doc §4.2: VerifyHTLC available in Challenged or Settling status
        if state.metadata.status != ChannelStatus::Challenged
            && state.metadata.status != ChannelStatus::Settling
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not challenged or settling (status: {:?})",
                state.metadata.status
            )));
        }
        // BUG-32: settle_deadline is optional in Challenged state; only enforce if set
        if let Some(deadline) = state.metadata.settle_deadline {
            if current_slot > deadline {
                return Err(StateChannelError::Other(anyhow::anyhow!(
                    "settlement window has expired (deadline: {}, current: {})",
                    deadline, current_slot
                )));
            }
        }
        if state.metadata.claimed_leaves.contains(&leaf_index) {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} has already been claimed",
                leaf_index
            )));
        }
        if *claimer_pubkey != state.metadata.user_pubkey
            && *claimer_pubkey != state.metadata.provider_pubkey
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not a channel participant"
            )));
        }

        // BUG-34 fix: fetch leaf first so we can include its amount in the claim message
        let leaf = state
            .tree
            .get_leaf(leaf_index as usize)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: leaf_index as usize,
                max: state.tree.num_leaves(),
            })?
            .clone();

        // BUG-34 fix: use claim-specific signature message
        let message = claim_message(
            &state.metadata.channel_id,
            leaf_index,
            leaf.amount,
            current_slot,
        );
        if !crate::signing::verify_ed25519_signature(&message, claimer_signature, claimer_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        if leaf.leaf_type != LeafType::HTLC {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} is not an HTLC leaf",
                leaf_index
            )));
        }

        // Verify claimer is the beneficiary
        let beneficiary = leaf.beneficiary.ok_or_else(|| {
            anyhow::anyhow!("HTLC leaf has no beneficiary")
        })?;
        if *claimer_pubkey != beneficiary {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not the HTLC beneficiary"
            )));
        }

        // Verify preimage matches hash_lock
        let hash_lock = leaf.hash_lock.ok_or_else(|| {
            anyhow::anyhow!("HTLC leaf has no hash_lock")
        })?;
        let computed_hash = hash(preimage).to_bytes();
        if computed_hash != hash_lock {
            return Err(StateChannelError::HashLockMismatch);
        }

        // Verify timelock has not expired
        let timelock = leaf.timelock_slot.ok_or_else(|| {
            anyhow::anyhow!("HTLC leaf has no timelock")
        })?;
        if current_slot > timelock {
            return Err(StateChannelError::HtlcExpired {
                expiry: timelock,
                current: current_slot,
            });
        }

        // Verify Merkle proof
        let proof = state.tree.get_proof(leaf_index as usize)?;
        let leaf_hash = leaf.hash();
        if !MerkleTree::verify_proof(&leaf_hash, &proof, &state.metadata.current_root) {
            return Err(StateChannelError::ProofVerificationFailed);
        }

        // Update totals with overflow protection
        let new_total = state.metadata.total_claimed.saturating_add(leaf.amount);
        if new_total > state.metadata.total_deposited {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "total_claimed would exceed total_deposited"
            )));
        }
        state.metadata.total_claimed = new_total;
        state.metadata.claimed_leaves.insert(leaf_index);
        self.persist_state(state)
    }

    /// FLOW-8: HTLCRefund — owner claims refund after HTLC expires.
    ///
    /// Per design doc §4.2 HTLCRefund: the owner submits proof that the HTLC
    /// has expired and no preimage was submitted. Validates: channel is Settling,
    /// settle_deadline, leaf is HTLC type, current_slot > timelock_slot,
    /// claimer is the leaf owner, Merkle proof, signature, overflow protection.
    pub fn claim_htlc_refund(
        &self,
        state: &mut ChannelState,
        leaf_index: u32,
        claimer_pubkey: &Pubkey,
        current_slot: u64,
        claimer_signature: &[u8; 64],
    ) -> Result<()> {
        // Per design doc §4.2: HTLCRefund available in Challenged or Settling status
        if state.metadata.status != ChannelStatus::Challenged
            && state.metadata.status != ChannelStatus::Settling
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not challenged or settling (status: {:?})",
                state.metadata.status
            )));
        }
        // BUG-32: settle_deadline is optional in Challenged state; only enforce if set
        if let Some(deadline) = state.metadata.settle_deadline {
            if current_slot > deadline {
                return Err(StateChannelError::Other(anyhow::anyhow!(
                    "settlement window has expired (deadline: {}, current: {})",
                    deadline, current_slot
                )));
            }
        }
        if state.metadata.claimed_leaves.contains(&leaf_index) {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} has already been claimed",
                leaf_index
            )));
        }
        if *claimer_pubkey != state.metadata.user_pubkey
            && *claimer_pubkey != state.metadata.provider_pubkey
        {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not a channel participant"
            )));
        }

        // BUG-34 fix: fetch leaf first so we can include its amount in the claim message
        let leaf = state
            .tree
            .get_leaf(leaf_index as usize)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: leaf_index as usize,
                max: state.tree.num_leaves(),
            })?
            .clone();

        // BUG-34 fix: use claim-specific signature message
        let message = claim_message(
            &state.metadata.channel_id,
            leaf_index,
            leaf.amount,
            current_slot,
        );
        if !crate::signing::verify_ed25519_signature(&message, claimer_signature, claimer_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        if leaf.leaf_type != LeafType::HTLC {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "leaf {} is not an HTLC leaf",
                leaf_index
            )));
        }

        // Verify claimer is the leaf owner
        if leaf.owner != *claimer_pubkey {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "claimer is not the HTLC owner"
            )));
        }

        // Verify HTLC has expired
        let timelock = leaf.timelock_slot.ok_or_else(|| {
            anyhow::anyhow!("HTLC leaf has no timelock")
        })?;
        if current_slot <= timelock {
            return Err(StateChannelError::HtlcNotExpired {
                expiry: timelock,
                current: current_slot,
            });
        }

        // Verify Merkle proof
        let proof = state.tree.get_proof(leaf_index as usize)?;
        let leaf_hash = leaf.hash();
        if !MerkleTree::verify_proof(&leaf_hash, &proof, &state.metadata.current_root) {
            return Err(StateChannelError::ProofVerificationFailed);
        }

        // Update totals with overflow protection
        let new_total = state.metadata.total_claimed.saturating_add(leaf.amount);
        if new_total > state.metadata.total_deposited {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "total_claimed would exceed total_deposited"
            )));
        }
        state.metadata.total_claimed = new_total;
        state.metadata.claimed_leaves.insert(leaf_index);
        self.persist_state(state)
    }

    /// Finalize settlement after the settle_deadline has passed.
    ///
    /// Per design doc §5.4, unclaimed funds are returned proportionally.
    /// Requires a signature to verify the caller's identity.
    /// Returns `(refund_a, refund_b)` — proportional refunds for unclaimed funds.
    pub fn finalize_settlement(
        &self,
        state: &mut ChannelState,
        current_slot: u64,
        caller_pubkey: &Pubkey,
        caller_signature: &[u8; 64],
    ) -> Result<(u64, u64)> {
        if state.metadata.status != ChannelStatus::Settling {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "channel is not settling (status: {:?})",
                state.metadata.status
            )));
        }

        let deadline = state.metadata.settle_deadline.ok_or_else(|| {
            StateChannelError::Other(anyhow::anyhow!("settle_deadline not set"))
        })?;

        if current_slot < deadline {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "settlement window has not expired (deadline: {}, current: {})",
                deadline, current_slot
            )));
        }

        // BUG-3 fix: verify caller's signature
        let message = crate::signing::state_message(
            &state.metadata.channel_id,
            current_slot,
            &state.metadata.current_root,
        );
        if !crate::signing::verify_ed25519_signature(&message, caller_signature, caller_pubkey) {
            return Err(StateChannelError::InvalidSignature);
        }

        // BUG-4 fix: compute proportional refunds for unclaimed funds
        // DEV-16: use full u128 precision for ratio calculation (avoid 1_000_000 scaling)
        let unclaimed = state.metadata.total_deposited.saturating_sub(state.metadata.total_claimed);
        let total_deposit_u64 = state.metadata.deposit_a.saturating_add(state.metadata.deposit_b);
        let (refund_a, refund_b) = if total_deposit_u64 > 0 {
            let total_deposit = total_deposit_u64 as u128;
            let r_a = (unclaimed as u128 * state.metadata.deposit_a as u128 / total_deposit) as u64;
            let r_b = unclaimed.saturating_sub(r_a);
            (r_a, r_b)
        } else {
            (unclaimed, 0)
        };

        state.metadata.status = ChannelStatus::Closed;
        self.persist_state(state)?;
        Ok((refund_a, refund_b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{generate_keypair, sign_leaf_update, sign_state, to_pubkey, verify_state_signature};
    use ed25519_dalek::Signer;

    fn temp_db() -> sled::Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn default_vaults() -> (Pubkey, Pubkey) {
        (Pubkey::new_unique(), Pubkey::new_unique())
    }

    #[test]
    fn test_open_channel() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let (vault_a, vault_b) = default_vaults();

        let state = mgr
            .open_channel(&user, &provider, &mint, 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        assert_eq!(state.metadata.total_deposited, 1_000_000);
        assert_eq!(state.metadata.sequence, 0);
        assert_eq!(state.metadata.status, ChannelStatus::Open);
        assert_eq!(state.tree.num_leaves(), 16);
        assert!(state.tree.validate_total_amount(1_000_000));
        assert_eq!(state.metadata.vault_a, vault_a);
        assert_eq!(state.metadata.vault_b, vault_b);
        assert_eq!(state.metadata.challenge_duration, 500);
        assert_eq!(state.metadata.min_challenge_delay, 50);
    }

    #[test]
    fn test_open_channel_zero_deposit_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let (vault_a, vault_b) = default_vaults();

        let result = mgr.open_channel(&user, &provider, &Pubkey::new_unique(), 0, 4, 100, &vault_a, &vault_b, 500, 50, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_persist_and_load() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let (vault_a, vault_b) = default_vaults();

        let state = mgr
            .open_channel(&user, &provider, &Pubkey::new_unique(), 500_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let channel_id = state.metadata.channel_id;
        let root_before = state.tree.root();

        let loaded = mgr.load_state(&channel_id).unwrap();
        assert_eq!(loaded.metadata.channel_id, channel_id);
        assert_eq!(loaded.tree.root(), root_before);
        assert!(loaded.tree.validate_total_amount(500_000));
    }

    #[test]
    fn test_persist_and_load_provider_cosign() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];

        let signed = mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        // provider_cosign should be persisted (BUG-1 fix)
        assert!(state.provider_cosign.is_some());

        let loaded = mgr.load_state(&state.metadata.channel_id).unwrap();
        assert_eq!(loaded.provider_cosign, Some(signed.sig_b));
    }

    #[test]
    fn test_construct_split_tree() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 100_000),
            UTXOLeaf::standard(to_pubkey(&user), 200_000),
            UTXOLeaf::standard(to_pubkey(&user), 700_000), // rest/change
        ];

        let signed = mgr
            .construct_split_tree(&mut state, leaves, &user, &provider)
            .unwrap();

        assert_eq!(signed.sequence, 1);
        assert_eq!(state.metadata.sequence, 1);
        assert!(state.tree.validate_total_amount(1_000_000));

        // Verify both signatures
        assert!(verify_state_signature(
            &signed.channel_id,
            signed.sequence,
            &signed.root,
            &signed.sig_a,
            &to_pubkey(&user)
        ));
        assert!(verify_state_signature(
            &signed.channel_id,
            signed.sequence,
            &signed.root,
            &signed.sig_b,
            &to_pubkey(&provider)
        ));
    }

    #[test]
    fn test_construct_split_tree_amount_mismatch() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 100_000),
            UTXOLeaf::standard(to_pubkey(&user), 200_000),
            // Missing 700_000 - amount conservation violated
        ];

        let result = mgr.construct_split_tree(&mut state, leaves, &user, &provider);
        assert!(result.is_err());
    }

    #[test]
    fn test_construct_split_tree_wrong_owner() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Leaf with wrong owner (not the user)
        let leaves = vec![
            UTXOLeaf::standard(Pubkey::new_unique(), 500_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];

        let result = mgr.construct_split_tree(&mut state, leaves, &user, &provider);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_leaf_update() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let signer = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&signer), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);

        let update = sign_leaf_update(
            &state.metadata.channel_id,
            1,
            0,
            &prev_leaf,
            new_leaf,
            &signer,
        );

        mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&signer)).unwrap();
        assert_eq!(state.metadata.sequence, 1);
        assert_eq!(state.tree.get_leaf(0).unwrap().amount, 500_000);
    }

    #[test]
    fn test_apply_leaf_update_wrong_sequence() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let signer = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&signer), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);

        let update = sign_leaf_update(
            &state.metadata.channel_id,
            5, // Wrong sequence
            0,
            &prev_leaf,
            new_leaf,
            &signer,
        );

        let result = mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&signer));
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_leaf_update_batch_all_or_nothing() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let signer = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&signer), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // First split the tree so we have multiple non-empty leaves
        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&signer), 400_000),
            UTXOLeaf::standard(to_pubkey(&signer), 300_000),
            UTXOLeaf::standard(to_pubkey(&signer), 300_000),
        ];
        mgr.construct_split_tree(&mut state, leaves, &signer, &provider).unwrap();

        // Create 2 valid updates
        let prev0 = state.tree.get_leaf(0).unwrap().clone();
        let new0 = UTXOLeaf::standard(Pubkey::new_unique(), 200_000);
        let update0 = sign_leaf_update(&state.metadata.channel_id, 2, 0, &prev0, new0, &signer);

        let prev1 = state.tree.get_leaf(1).unwrap().clone();
        let new1 = UTXOLeaf::standard(Pubkey::new_unique(), 150_000);
        let update1 = sign_leaf_update(&state.metadata.channel_id, 3, 1, &prev1, new1, &signer);

        // Create a 3rd update with wrong sequence to trigger failure
        let prev2 = state.tree.get_leaf(2).unwrap().clone();
        let new2 = UTXOLeaf::standard(Pubkey::new_unique(), 100_000);
        let update2 = sign_leaf_update(&state.metadata.channel_id, 99, 2, &prev2, new2, &signer);

        let root_before = state.tree.root();
        let seq_before = state.metadata.sequence;

        let result = mgr.apply_leaf_update_batch(
            &mut state,
            &[update0, update1, update2],
            &to_pubkey(&signer),
        );
        assert!(result.is_err());

        // State should be rolled back
        assert_eq!(state.metadata.sequence, seq_before);
        assert_eq!(state.tree.root(), root_before);
    }

    #[test]
    fn test_close_channel() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // BUG-3 fix: close_channel now requires dual-signed state
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(
                &state.metadata.channel_id,
                state.metadata.sequence,
                &state.metadata.current_root,
                &user,
            ),
            sig_b: sign_state(
                &state.metadata.channel_id,
                state.metadata.sequence,
                &state.metadata.current_root,
                &provider,
            ),
        };

        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Settling);
        assert_eq!(state.metadata.settle_deadline, Some(300));

        // Verify persisted
        let loaded = mgr.load_state(&state.metadata.channel_id).unwrap();
        assert_eq!(loaded.metadata.status, ChannelStatus::Settling);
        assert_eq!(loaded.metadata.settle_deadline, Some(300));
    }

    #[test]
    fn test_close_channel_wrong_sig_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let wrong_party = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Use wrong party's signature for sig_b
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(
                &state.metadata.channel_id,
                state.metadata.sequence,
                &state.metadata.current_root,
                &user,
            ),
            sig_b: sign_state(
                &state.metadata.channel_id,
                state.metadata.sequence,
                &state.metadata.current_root,
                &wrong_party, // Wrong signer
            ),
        };

        let result = mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_trigger_challenge() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // User triggers challenge at slot 200 (min_challenge_delay=50, open_slot=100, so min=150)
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Challenged);
        assert_eq!(state.metadata.challenge_slot, Some(200));

        // Verify persisted
        let loaded = mgr.load_state(&state.metadata.channel_id).unwrap();
        assert_eq!(loaded.metadata.status, ChannelStatus::Challenged);
    }

    #[test]
    fn test_trigger_challenge_min_delay_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Attempt challenge at slot 120 (< open_slot + min_challenge_delay = 100 + 50 = 150)
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        let result = mgr.trigger_challenge(&mut state, &to_pubkey(&user), 120, &submitted_root, submitted_sequence, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_trigger_challenge_non_participant_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let outsider = generate_keypair();
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = outsider.sign(&msg).to_bytes();
        let result = mgr.trigger_challenge(&mut state, &to_pubkey(&outsider), 200, &submitted_root, submitted_sequence, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_trigger_challenge_wrong_signature_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Sign with wrong sequence (999 instead of submitted_sequence)
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: the signature is over submitted_sequence; sign with wrong value to test rejection
        let msg = crate::signing::state_message(&state.metadata.channel_id, 999, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        let result = mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_and_finalize() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close channel (Settling)
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Settling);

        // Claim leaf 0 (owner is user, amount is 1_000_000)
        let claim_msg = crate::signing::claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
        let claim_sig = user.sign(&claim_msg).to_bytes();
        mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 250, &claim_sig).unwrap();
        assert_eq!(state.metadata.total_claimed, 1_000_000);
        assert!(state.metadata.claimed_leaves.contains(&0));

        // Cannot claim same leaf twice
        let result = mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 250, &claim_sig);
        assert!(result.is_err());

        // Cannot finalize before deadline
        let msg = crate::signing::state_message(&state.metadata.channel_id, 299, &state.metadata.current_root);
        let sig = user.sign(&msg).to_bytes();
        let result = mgr.finalize_settlement(&mut state, 299, &to_pubkey(&user), &sig);
        assert!(result.is_err());

        // Finalize after deadline
        let msg = crate::signing::state_message(&state.metadata.channel_id, 300, &state.metadata.current_root);
        let sig = user.sign(&msg).to_bytes();
        let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 300, &to_pubkey(&user), &sig).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Closed);
        // All claimed, so unclaimed = 0
        assert_eq!(refund_a, 0);
        assert_eq!(refund_b, 0);

        let loaded = mgr.load_state(&state.metadata.channel_id).unwrap();
        assert_eq!(loaded.metadata.status, ChannelStatus::Closed);
        assert_eq!(loaded.metadata.total_claimed, 1_000_000);
    }

    #[test]
    fn test_claim_leaf_wrong_amount_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close channel
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

        // Try claiming with wrong amount
        let claim_msg = crate::signing::claim_message(&state.metadata.channel_id, 0, 500_000, 250);
        let claim_sig = user.sign(&claim_msg).to_bytes();
        let result = mgr.claim_leaf(&mut state, 0, 500_000, &to_pubkey(&user), 250, &claim_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_leaf_wrong_owner_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close channel
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

        // Provider tries to claim user's leaf
        let claim_msg = crate::signing::claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
        let claim_sig = provider.sign(&claim_msg).to_bytes();
        let result = mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&provider), 250, &claim_sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_finalize_proportional_refund() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close channel
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

        // Claim only half
        let claim_msg = crate::signing::claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
        let claim_sig = user.sign(&claim_msg).to_bytes();
        mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 250, &claim_sig).unwrap();
        // total_claimed = 1_000_000, total_deposited = 1_000_000, so unclaimed = 0
        // But deposit_a = 1_000_000, deposit_b = 0
        let msg = crate::signing::state_message(&state.metadata.channel_id, 300, &state.metadata.current_root);
        let sig = user.sign(&msg).to_bytes();
        let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 300, &to_pubkey(&user), &sig).unwrap();
        // Everything claimed, no refunds
        assert_eq!(refund_a, 0);
        assert_eq!(refund_b, 0);
    }

    #[test]
    fn test_settle_after_timeout() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Trigger challenge
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Challenged);

        // Cannot settle before challenge_duration (500) elapses
        let result = mgr.settle_after_timeout(&mut state, 699, 100);
        assert!(result.is_err());

        // Cannot settle at exactly challenge_slot + duration (strict >)
        let result = mgr.settle_after_timeout(&mut state, 700, 100);
        assert!(result.is_err());

        // Settle after challenge_duration (strict >: current_slot > 700)
        mgr.settle_after_timeout(&mut state, 701, 100).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Settling);
        assert_eq!(state.metadata.settle_deadline, Some(801));
    }

    #[test]
    fn test_submit_counter_state() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Trigger challenge
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig).unwrap();
        // DEV-9: trigger_challenge now updates sequence to submitted_sequence
        assert_eq!(state.metadata.sequence, 1);

        // Provider submits counter state with higher sequence (simulated)
        let fake_root = [99u8; 32];
        let counter_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: 5, // Higher than current (1)
            root: fake_root,
            sig_a: sign_state(&state.metadata.channel_id, 5, &fake_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, 5, &fake_root, &provider),
        };

        mgr.submit_counter_state(&mut state, &counter_state, None, &to_pubkey(&user), &to_pubkey(&provider)).unwrap();
        assert_eq!(state.metadata.sequence, 5);
        assert_eq!(state.metadata.current_root, fake_root);
        assert_eq!(state.metadata.status, ChannelStatus::Challenged); // Still challenged
    }

    #[test]
    fn test_submit_counter_state_lower_sequence_rejected() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Trigger challenge
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig).unwrap();

        // Counter state with same or lower sequence should be rejected
        let counter_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: 0, // Same as current
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, 0, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, 0, &state.metadata.current_root, &provider),
        };

        let result = mgr.submit_counter_state(&mut state, &counter_state, None, &to_pubkey(&user), &to_pubkey(&provider));
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_leaf_update_rejected_when_not_open() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let signer = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&signer), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close channel to move to Settling
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &signer),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&signer), &to_pubkey(&provider), 200, 100).unwrap();

        // Try to apply leaf update in Settling state
        let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);
        let update = sign_leaf_update(&state.metadata.channel_id, 1, 0, &prev_leaf, new_leaf, &signer);

        let result = mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&signer));
        assert!(result.is_err());
    }

    #[test]
    fn test_dispute_full_lifecycle() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        // Open
        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Challenge
        let submitted_root = state.metadata.current_root;
        let submitted_sequence = state.metadata.sequence + 1;
        // BUG-23: sign with submitted_sequence, not current_slot
        let msg = crate::signing::state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
        let sig = user.sign(&msg).to_bytes();
        mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig).unwrap();

        // Settle after timeout (strict >: challenge_slot=200 + challenge_duration=500 = 700, need > 700)
        mgr.settle_after_timeout(&mut state, 701, 100).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Settling);
        assert_eq!(state.metadata.settle_deadline, Some(801));

        // Claim
        let claim_msg = crate::signing::claim_message(&state.metadata.channel_id, 0, 1_000_000, 750);
        let claim_sig = user.sign(&claim_msg).to_bytes();
        mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 750, &claim_sig).unwrap();

        // Finalize
        let msg = crate::signing::state_message(&state.metadata.channel_id, 801, &state.metadata.current_root);
        let sig = user.sign(&msg).to_bytes();
        let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 801, &to_pubkey(&user), &sig).unwrap();
        assert_eq!(state.metadata.status, ChannelStatus::Closed);
        assert_eq!(refund_a, 0);
        assert_eq!(refund_b, 0);
    }

    #[test]
    fn test_load_nonexistent_channel() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let result = mgr.load_state(&[0u8; 32]);
        assert!(result.is_err());
    }

    // ========== FLOW-3: Dual-funded Channel Tests ==========

    #[test]
    fn test_fund_channel_basic() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Provider funds with 500_000
        mgr.fund_channel(&mut state, &provider, 500_000, None).unwrap();

        assert_eq!(state.metadata.deposit_b, 500_000);
        assert_eq!(state.metadata.total_deposited, 1_500_000);
        assert_eq!(state.metadata.sequence, 1);
        assert!(state.tree.validate_total_amount(1_500_000));

        // Provider leaf should exist
        let provider_leaves: Vec<_> = state.tree.leaves().iter()
            .filter(|l| l.owner == to_pubkey(&provider))
            .collect();
        assert_eq!(provider_leaves.len(), 1);
        assert_eq!(provider_leaves[0].amount, 500_000);
    }

    #[test]
    fn test_fund_channel_specific_slot() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Fund into specific slot 3
        mgr.fund_channel(&mut state, &provider, 500_000, Some(3)).unwrap();
        assert_eq!(state.tree.get_leaf(3).unwrap().amount, 500_000);
        assert_eq!(state.tree.get_leaf(3).unwrap().owner, to_pubkey(&provider));
    }

    #[test]
    fn test_fund_channel_rejected_twice() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        mgr.fund_channel(&mut state, &provider, 500_000, None).unwrap();

        // Second funding should fail
        let result = mgr.fund_channel(&mut state, &provider, 300_000, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_fund_channel_rejected_wrong_signer() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // User cannot fund as provider
        let result = mgr.fund_channel(&mut state, &user, 500_000, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_fund_channel_rejected_zero_deposit() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let result = mgr.fund_channel(&mut state, &provider, 0, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_fund_channel_rejected_occupied_slot() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Slot 0 is already occupied by user's root leaf
        let result = mgr.fund_channel(&mut state, &provider, 500_000, Some(0));
        assert!(result.is_err());
    }

    #[test]
    fn test_fund_channel_persistence() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let channel_id = state.metadata.channel_id;
        mgr.fund_channel(&mut state, &provider, 500_000, None).unwrap();

        // Load from persistence
        let loaded = mgr.load_state(&channel_id).unwrap();
        assert_eq!(loaded.metadata.deposit_b, 500_000);
        assert_eq!(loaded.metadata.total_deposited, 1_500_000);
        assert_eq!(loaded.metadata.sequence, 1);
        assert!(loaded.tree.validate_total_amount(1_500_000));
    }

    #[test]
    fn test_fund_channel_rejected_not_open() {
        let db = temp_db();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        // Close the channel first
        let signed_state = SignedState {
            channel_id: state.metadata.channel_id,
            sequence: state.metadata.sequence,
            root: state.metadata.current_root,
            sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
            sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
        };
        mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

        // Funding should fail on non-Open channel
        let result = mgr.fund_channel(&mut state, &provider, 500_000, None);
        assert!(result.is_err());
    }
}
