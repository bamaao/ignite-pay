use crate::error::{Result, StateChannelError};
use crate::merkle::MerkleTree;
use crate::signing::{sign_leaf_update, to_pubkey};
use crate::types::{LeafUpdate, UTXOLeaf};
use ed25519_dalek::Keypair;
use solana_pubkey::Pubkey;

/// Split an amount from a "rest" (change) leaf into a target empty slot.
///
/// Operation order per design doc §3.2.2: deduct from Rest FIRST, then create target.
/// This ensures at every intermediate sequence `sum(leaves) <= total_deposited`,
/// preventing any intermediate state from appearing to have excess funds.
/// Returns two signed LeafUpdates: [rest_deduction, target_creation].
pub fn split_from_rest(
    channel_id: &[u8; 32],
    rest_idx: usize,
    target_idx: usize,
    amount: u64,
    sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<(Vec<LeafUpdate>, u64)> {
    let rest_leaf = tree
        .get_leaf(rest_idx)
        .ok_or(StateChannelError::LeafIndexOutOfBounds {
            index: rest_idx,
            max: tree.num_leaves(),
        })?
        .clone();

    if rest_leaf.amount < amount {
        return Err(StateChannelError::InsufficientBalance {
            required: amount,
            available: rest_leaf.amount,
        });
    }

    // CODE-4 fix: verify the signer owns the rest leaf
    if rest_leaf.owner != to_pubkey(signer) {
        return Err(StateChannelError::Other(anyhow::anyhow!(
            "signer does not own the rest leaf"
        )));
    }

    let target_leaf = tree
        .get_leaf(target_idx)
        .ok_or(StateChannelError::LeafIndexOutOfBounds {
            index: target_idx,
            max: tree.num_leaves(),
        })?
        .clone();

    if !target_leaf.is_empty() {
        return Err(StateChannelError::LeafSlotOccupied);
    }

    let mut updates = Vec::with_capacity(2);
    let mut seq = sequence;

    // ISSUE-1 fix: follow design doc §3.2.2 order — deduct from Rest first,
    // then create target. This preserves sum(leaves) <= total_deposited at every
    // intermediate sequence.

    // Step 1: Deduct from rest (decreases total)
    let updated_rest = UTXOLeaf::standard(rest_leaf.owner, rest_leaf.amount.saturating_sub(amount));
    let update1 = sign_leaf_update(
        channel_id,
        seq,
        rest_idx as u32,
        &rest_leaf,
        updated_rest.clone(),
        signer,
    );
    tree.update_leaf(rest_idx, updated_rest)?;
    updates.push(update1);
    seq += 1;

    // Step 2: Create target — use signer's pubkey, not the empty leaf's default
    // Re-read the target leaf since it hasn't changed (different index from rest)
    let new_target = UTXOLeaf::standard(to_pubkey(signer), amount);
    let update2 = sign_leaf_update(
        channel_id,
        seq,
        target_idx as u32,
        &target_leaf,
        new_target.clone(),
        signer,
    );
    tree.update_leaf(target_idx, new_target)?;
    updates.push(update2);
    seq += 1;

    Ok((updates, seq))
}

/// Merge multiple spent source leaves into a single target leaf.
///
/// Verifies all source leaves are owned by the signer, then updates the target
/// (adding combined amount) before clearing source leaves. This order preserves
/// the amount conservation invariant at every intermediate sequence.
/// Returns N+1 signed LeafUpdates: [set_target, clear_src_1, clear_src_2, ...].
pub fn merge_spent_leaves(
    channel_id: &[u8; 32],
    source_indices: &[usize],
    target_idx: usize,
    sequence: u64,
    tree: &mut MerkleTree,
    signer: &Keypair,
) -> Result<(Vec<LeafUpdate>, u64)> {
    let signer_pubkey = to_pubkey(signer);
    let mut total_amount = 0u64;
    let mut source_leaves = Vec::with_capacity(source_indices.len());

    for &idx in source_indices {
        let leaf = tree
            .get_leaf(idx)
            .ok_or(StateChannelError::LeafIndexOutOfBounds {
                index: idx,
                max: tree.num_leaves(),
            })?
            .clone();

        // BUG-3 fix: verify ownership — only the signer can merge their own leaves
        if leaf.owner != signer_pubkey {
            return Err(StateChannelError::Other(anyhow::anyhow!(
                "cannot merge leaf {}: owner does not match signer",
                idx
            )));
        }

        total_amount = total_amount.saturating_add(leaf.amount);
        source_leaves.push((idx, leaf));
    }

    // DEV-12: target index cannot be one of the source indices
    if source_indices.contains(&target_idx) {
        return Err(StateChannelError::Other(anyhow::anyhow!(
            "target index {} cannot be one of the source indices",
            target_idx
        )));
    }

    let target_leaf = tree
        .get_leaf(target_idx)
        .ok_or(StateChannelError::LeafIndexOutOfBounds {
            index: target_idx,
            max: tree.num_leaves(),
        })?
        .clone();

    let mut updates = Vec::with_capacity(source_indices.len() + 1);
    let mut seq = sequence;

    // BUG-2 fix: set target FIRST (adds amount), then clear sources (removes amount).
    // This preserves sum(leaves) >= total_deposited at every intermediate sequence.

    // Step 1: Set target leaf with combined amount
    let target_owner = if target_leaf.is_empty() {
        signer_pubkey
    } else {
        target_leaf.owner
    };

    let new_target = UTXOLeaf::standard(target_owner, target_leaf.amount.saturating_add(total_amount));
    let update = sign_leaf_update(
        channel_id,
        seq,
        target_idx as u32,
        &target_leaf,
        new_target.clone(),
        signer,
    );
    tree.update_leaf(target_idx, new_target)?;
    updates.push(update);
    seq += 1;

    // Step 2: Clear all source leaves
    for (idx, prev_leaf) in &source_leaves {
        let cleared = UTXOLeaf::empty();
        let update = sign_leaf_update(
            channel_id,
            seq,
            *idx as u32,
            prev_leaf,
            cleared.clone(),
            signer,
        );
        tree.update_leaf(*idx, cleared)?;
        updates.push(update);
        seq += 1;
    }

    Ok((updates, seq))
}

// ============================================================================
// UTXO Denomination Strategy (Design Doc §3.1.2)
// ============================================================================

/// UTXO denomination tier: count × amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenominationTier {
    pub count: usize,
    pub amount: u64,
}

/// Strategy for splitting a deposit into UTXO leaves at channel opening.
///
/// Design doc §3.1.2 defines three strategies:
/// - **Uniform**: all leaves same amount + one Rest leaf
/// - **Mixed**: multiple tiers of different amounts + one Rest leaf
/// - **RestFirst**: few small leaves + one large Rest leaf
pub enum DenominationStrategy {
    /// Uniform split: `count` leaves of `amount` each, plus a Rest leaf.
    /// Best for fixed-price scenarios.
    Uniform { count: usize, amount: u64 },
    /// Mixed denomination: explicit tiers, plus an auto-computed Rest leaf.
    /// Best for variable-price scenarios.
    Mixed { tiers: Vec<DenominationTier> },
    /// Rest-first: `small_count` leaves of `small_amount` each, plus a large Rest leaf.
    /// Best for large-value, low-frequency scenarios.
    RestFirst { small_count: usize, small_amount: u64 },
}

impl DenominationStrategy {
    /// Generate UTXO leaves for the given strategy and total deposit.
    ///
    /// Returns a vector of `UTXOLeaf::standard(owner, amount)` leaves whose
    /// amounts sum to `deposit`. The last leaf is the Rest (change) leaf.
    ///
    /// Returns an error if the tier amounts exceed the deposit.
    pub fn generate_leaves(&self, owner: Pubkey, deposit: u64) -> Result<Vec<UTXOLeaf>> {
        match self {
            DenominationStrategy::Uniform { count, amount } => {
                let total_in_tiers = (*amount as u64)
                    .checked_mul(*count as u64)
                    .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("amount overflow")))?;
                if total_in_tiers > deposit {
                    return Err(StateChannelError::Other(anyhow::anyhow!(
                        "uniform tier total {} exceeds deposit {}",
                        total_in_tiers,
                        deposit
                    )));
                }
                let rest = deposit - total_in_tiers;
                let mut leaves = Vec::with_capacity(count + 1);
                for _ in 0..*count {
                    leaves.push(UTXOLeaf::standard(owner, *amount));
                }
                leaves.push(UTXOLeaf::standard(owner, rest));
                Ok(leaves)
            }
            DenominationStrategy::Mixed { tiers } => {
                let mut total_in_tiers = 0u64;
                let mut leaves = Vec::new();
                for tier in tiers {
                    let tier_total = tier
                        .amount
                        .checked_mul(tier.count as u64)
                        .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("tier overflow")))?;
                    total_in_tiers = total_in_tiers
                        .checked_add(tier_total)
                        .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("total overflow")))?;
                    if total_in_tiers > deposit {
                        return Err(StateChannelError::Other(anyhow::anyhow!(
                            "mixed tier total {} exceeds deposit {}",
                            total_in_tiers,
                            deposit
                        )));
                    }
                    for _ in 0..tier.count {
                        leaves.push(UTXOLeaf::standard(owner, tier.amount));
                    }
                }
                let rest = deposit - total_in_tiers;
                leaves.push(UTXOLeaf::standard(owner, rest));
                Ok(leaves)
            }
            DenominationStrategy::RestFirst {
                small_count,
                small_amount,
            } => {
                let total_small = (*small_amount as u64)
                    .checked_mul(*small_count as u64)
                    .ok_or_else(|| StateChannelError::Other(anyhow::anyhow!("amount overflow")))?;
                if total_small > deposit {
                    return Err(StateChannelError::Other(anyhow::anyhow!(
                        "small leaf total {} exceeds deposit {}",
                        total_small,
                        deposit
                    )));
                }
                let rest = deposit - total_small;
                let mut leaves = Vec::with_capacity(small_count + 1);
                for _ in 0..*small_count {
                    leaves.push(UTXOLeaf::standard(owner, *small_amount));
                }
                leaves.push(UTXOLeaf::standard(owner, rest));
                Ok(leaves)
            }
        }
    }
}

#[cfg(test)]
mod denomination_tests {
    use super::*;
    use solana_pubkey::Pubkey;

    #[test]
    fn test_uniform_strategy() {
        let owner = Pubkey::new_unique();
        let strategy = DenominationStrategy::Uniform {
            count: 3,
            amount: 100,
        };
        let leaves = strategy.generate_leaves(owner, 1000).unwrap();
        assert_eq!(leaves.len(), 4); // 3 uniform + 1 rest
        assert_eq!(leaves[0].amount, 100);
        assert_eq!(leaves[1].amount, 100);
        assert_eq!(leaves[2].amount, 100);
        assert_eq!(leaves[3].amount, 700); // rest
        assert_eq!(leaves.iter().map(|l| l.amount).sum::<u64>(), 1000);
    }

    #[test]
    fn test_uniform_exceeds_deposit() {
        let owner = Pubkey::new_unique();
        let strategy = DenominationStrategy::Uniform {
            count: 10,
            amount: 200,
        };
        let result = strategy.generate_leaves(owner, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_mixed_strategy() {
        let owner = Pubkey::new_unique();
        let strategy = DenominationStrategy::Mixed {
            tiers: vec![
                DenominationTier { count: 50, amount: 10_000 },
                DenominationTier { count: 10, amount: 50_000 },
                DenominationTier { count: 5, amount: 100_000 },
            ],
        };
        let deposit = 10_000_000;
        let leaves = strategy.generate_leaves(owner, deposit).unwrap();
        // 50 + 10 + 5 + 1(rest) = 66
        assert_eq!(leaves.len(), 66);
        assert_eq!(leaves.iter().map(|l| l.amount).sum::<u64>(), deposit);
        // Last leaf is rest
        let tier_total = 50 * 10_000 + 10 * 50_000 + 5 * 100_000;
        assert_eq!(leaves.last().unwrap().amount, deposit - tier_total as u64);
    }

    #[test]
    fn test_rest_first_strategy() {
        let owner = Pubkey::new_unique();
        let strategy = DenominationStrategy::RestFirst {
            small_count: 10,
            small_amount: 100_000,
        };
        let deposit = 10_000_000;
        let leaves = strategy.generate_leaves(owner, deposit).unwrap();
        assert_eq!(leaves.len(), 11); // 10 small + 1 rest
        assert_eq!(leaves[0].amount, 100_000);
        assert_eq!(leaves[10].amount, 9_000_000); // rest
        assert_eq!(leaves.iter().map(|l| l.amount).sum::<u64>(), deposit);
    }

    #[test]
    fn test_zero_rest_strategy() {
        let owner = Pubkey::new_unique();
        let strategy = DenominationStrategy::Uniform {
            count: 4,
            amount: 250_000,
        };
        let leaves = strategy.generate_leaves(owner, 1_000_000).unwrap();
        assert_eq!(leaves.len(), 5); // 4 uniform + 1 rest (0)
        assert_eq!(leaves[4].amount, 0); // zero rest
        assert_eq!(leaves.iter().map(|l| l.amount).sum::<u64>(), 1_000_000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::ChannelManager;
    use crate::signing::{generate_keypair, to_pubkey};
    use solana_pubkey::Pubkey;

    fn setup_split_tree() -> (MerkleTree, [u8; 32], Keypair, Keypair) {
        let user = generate_keypair();
        let provider = generate_keypair();
        let db_dir = tempfile::tempdir().unwrap();
        let db = sled::open(db_dir.path()).unwrap();
        let mgr = ChannelManager::new(db).unwrap();

        let vault_a = Pubkey::new_unique();
        let vault_b = Pubkey::new_unique();
        let mut state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        let leaves = vec![
            UTXOLeaf::standard(to_pubkey(&user), 200_000),
            UTXOLeaf::standard(to_pubkey(&user), 300_000),
            UTXOLeaf::standard(to_pubkey(&user), 500_000),
        ];
        mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

        (state.tree, state.metadata.channel_id, user, provider)
    }

    #[test]
    fn test_split_from_rest() {
        let (mut tree, channel_id, signer, _) = setup_split_tree();

        let seq = 2u64;
        let (updates, final_seq) = split_from_rest(
            &channel_id,
            2,
            3,
            100_000,
            seq,
            &mut tree,
            &signer,
        )
        .unwrap();

        assert_eq!(updates.len(), 2);
        assert_eq!(final_seq, seq + 2);
        assert_eq!(tree.get_leaf(2).unwrap().amount, 400_000);
        assert_eq!(tree.get_leaf(3).unwrap().amount, 100_000);
        // BUG-1 fix: new leaf owner should be the signer, not default
        assert_eq!(tree.get_leaf(3).unwrap().owner, to_pubkey(&signer));
        assert_eq!(tree.total_amount(), 1_000_000);
    }

    #[test]
    fn test_split_from_rest_insufficient() {
        let (mut tree, channel_id, signer, _) = setup_split_tree();

        let result = split_from_rest(&channel_id, 2, 3, 999_999_999, 2, &mut tree, &signer);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_from_rest_occupied_target() {
        let (mut tree, channel_id, signer, _) = setup_split_tree();

        let result = split_from_rest(&channel_id, 2, 0, 100_000, 2, &mut tree, &signer);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_from_rest_wrong_signer() {
        let (mut tree, channel_id, _, _) = setup_split_tree();

        let wrong_signer = generate_keypair();
        let result = split_from_rest(&channel_id, 2, 3, 100_000, 2, &mut tree, &wrong_signer);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_spent_leaves() {
        let (mut tree, channel_id, signer, _) = setup_split_tree();

        let seq = 2u64;

        let (updates, final_seq) = merge_spent_leaves(
            &channel_id,
            &[0, 1],
            2,
            seq,
            &mut tree,
            &signer,
        )
        .unwrap();

        // 1 target update + 2 clears = 3 updates
        assert_eq!(updates.len(), 3);
        assert_eq!(final_seq, seq + 3);
        assert!(tree.get_leaf(0).unwrap().is_empty());
        assert!(tree.get_leaf(1).unwrap().is_empty());
        // 500_000 (existing) + 200_000 + 300_000 = 1_000_000
        assert_eq!(tree.get_leaf(2).unwrap().amount, 1_000_000);
        assert_eq!(tree.total_amount(), 1_000_000);
    }

    #[test]
    fn test_merge_into_empty_slot() {
        let (mut tree, channel_id, signer, _) = setup_split_tree();

        let seq = 2u64;

        let (updates, _) = merge_spent_leaves(
            &channel_id,
            &[0, 1],
            5,
            seq,
            &mut tree,
            &signer,
        )
        .unwrap();

        assert_eq!(updates.len(), 3);
        assert!(tree.get_leaf(0).unwrap().is_empty());
        assert!(tree.get_leaf(1).unwrap().is_empty());
        assert_eq!(tree.get_leaf(5).unwrap().amount, 500_000); // 200_000 + 300_000
        assert_eq!(tree.get_leaf(5).unwrap().owner, to_pubkey(&signer));
        assert_eq!(tree.total_amount(), 1_000_000);
    }

    #[test]
    fn test_merge_wrong_owner() {
        let (mut tree, channel_id, _, _) = setup_split_tree();

        let wrong_signer = generate_keypair();
        let result = merge_spent_leaves(
            &channel_id,
            &[0, 1],
            2,
            2,
            &mut tree,
            &wrong_signer,
        );
        assert!(result.is_err());
    }
}
