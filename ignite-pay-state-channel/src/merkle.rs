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
use crate::types::UTXOLeaf;
use solana_program::hash::hashv;

/// Off-chain binary Merkle tree with sorted-pair hashing.
///
/// Uses the same hashv(&[min, max]) pattern as on-chain
/// `compression.rs:verify_proof_locally` to ensure off-chain/on-chain compatibility.
pub struct MerkleTree {
    /// The leaf data.
    leaves: Vec<UTXOLeaf>,
    /// Cached hashes of each leaf.
    leaf_hashes: Vec<[u8; 32]>,
    /// Internal nodes stored level-by-level, bottom to top.
    /// nodes[0] = leaf hashes (level 0), nodes[1] = parents (level 1), etc.
    /// The last level has exactly one element: the root.
    nodes: Vec<Vec<[u8; 32]>>,
    /// Tree depth (number of levels above leaves).
    tree_depth: usize,
    /// Maximum number of leaves = 2^tree_depth.
    max_leaves: usize,
}

impl std::fmt::Debug for MerkleTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MerkleTree")
            .field("tree_depth", &self.tree_depth)
            .field("max_leaves", &self.max_leaves)
            .field("num_leaves", &self.leaves.len())
            .finish()
    }
}

impl MerkleTree {
    /// Build a new Merkle tree from a set of leaves.
    ///
    /// Pads with empty leaves to fill 2^tree_depth slots, then builds
    /// level-by-level from bottom to top.
    pub fn new(leaves: Vec<UTXOLeaf>, tree_depth: usize) -> Result<Self> {
        let max_leaves = 1usize << tree_depth;
        if leaves.len() > max_leaves {
            return Err(StateChannelError::LeafIndexOutOfBounds {
                index: leaves.len(),
                max: max_leaves,
            });
        }

        // Pad with empty leaves
        let mut all_leaves = leaves;
        while all_leaves.len() < max_leaves {
            all_leaves.push(UTXOLeaf::empty());
        }

        // Compute leaf hashes
        let leaf_hashes: Vec<[u8; 32]> = all_leaves.iter().map(|l| l.hash()).collect();

        // Build internal nodes level-by-level
        let mut nodes = Vec::with_capacity(tree_depth + 1);
        nodes.push(leaf_hashes.clone());

        let mut current_level = leaf_hashes.clone();
        for _ in 0..tree_depth {
            let mut parent_level = Vec::with_capacity(current_level.len() / 2);
            for pair in current_level.chunks(2) {
                let (left, right) = if pair[0] < pair[1] {
                    (pair[0], pair[1])
                } else {
                    (pair[1], pair[0])
                };
                parent_level.push(hashv(&[&left, &right]).to_bytes());
            }
            current_level = parent_level.clone();
            nodes.push(parent_level);
        }

        Ok(Self {
            leaves: all_leaves,
            leaf_hashes,
            nodes,
            tree_depth,
            max_leaves,
        })
    }

    /// Return the current Merkle root hash.
    pub fn root(&self) -> [u8; 32] {
        self.nodes
            .last()
            .and_then(|level| level.first())
            .copied()
            .unwrap_or([0u8; 32])
    }

    /// Get a reference to the leaf at the given index.
    pub fn get_leaf(&self, index: usize) -> Option<&UTXOLeaf> {
        self.leaves.get(index)
    }

    /// Get the hash of the leaf at the given index.
    pub fn get_leaf_hash(&self, index: usize) -> Option<[u8; 32]> {
        self.leaf_hashes.get(index).copied()
    }

    /// Get the total number of leaf slots.
    pub fn num_leaves(&self) -> usize {
        self.max_leaves
    }

    /// Get the tree depth.
    pub fn tree_depth(&self) -> usize {
        self.tree_depth
    }

    /// Update a leaf at the given index and recompute the root in O(depth) time.
    pub fn update_leaf(&mut self, index: usize, new_leaf: UTXOLeaf) -> Result<[u8; 32]> {
        if index >= self.max_leaves {
            return Err(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.max_leaves,
            });
        }

        let new_hash = new_leaf.hash();
        self.leaves[index] = new_leaf;
        self.leaf_hashes[index] = new_hash;

        // Update level 0 node
        self.nodes[0][index] = new_hash;

        // Recompute path from leaf to root
        let mut current_index = index;
        for level in 0..self.tree_depth {
            let sibling_index = current_index ^ 1;
            let parent_index = current_index / 2;

            let (left, right) = {
                let current_val = self.nodes[level][current_index];
                let sibling_val = self.nodes[level][sibling_index];
                if current_val < sibling_val {
                    (current_val, sibling_val)
                } else {
                    (sibling_val, current_val)
                }
            };

            let parent_hash = hashv(&[&left, &right]).to_bytes();
            self.nodes[level + 1][parent_index] = parent_hash;
            current_index = parent_index;
        }

        Ok(self.root())
    }

    /// Generate a Merkle proof (sibling hashes) from leaf to root.
    pub fn get_proof(&self, index: usize) -> Result<Vec<[u8; 32]>> {
        if index >= self.max_leaves {
            return Err(StateChannelError::LeafIndexOutOfBounds {
                index,
                max: self.max_leaves,
            });
        }

        let mut proof = Vec::with_capacity(self.tree_depth);
        let mut current_index = index;

        for level in 0..self.tree_depth {
            let sibling_index = current_index ^ 1;
            proof.push(self.nodes[level][sibling_index]);
            current_index /= 2;
        }

        Ok(proof)
    }

    /// Verify a Merkle proof against an expected root.
    ///
    /// Uses the same sorted-pair hashing as `compression.rs:verify_proof_locally`:
    /// At each level, sort the two hashes, then hashv(&[min, max]).
    pub fn verify_proof(leaf_hash: &[u8; 32], proof: &[[u8; 32]], root: &[u8; 32]) -> bool {
        let mut current = *leaf_hash;
        for sibling in proof {
            let (left, right) = if current < *sibling {
                (current, *sibling)
            } else {
                (*sibling, current)
            };
            current = hashv(&[&left, &right]).to_bytes();
        }
        current == *root
    }

    /// Validate that the sum of all leaf amounts equals the expected total.
    pub fn validate_total_amount(&self, expected: u64) -> bool {
        let actual = self.total_amount();
        actual == expected
    }

    /// Get total amount across all non-empty leaves.
    /// Uses saturating_add to prevent overflow panics in debug mode.
    pub fn total_amount(&self) -> u64 {
        self.leaves.iter().map(|l| l.amount).fold(0u64, |acc, x| acc.saturating_add(x))
    }

    /// Find indices of empty leaf slots (amount == 0).
    pub fn available_slots(&self) -> Vec<usize> {
        self.leaves
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_empty())
            .map(|(i, _)| i)
            .collect()
    }

    /// Get a reference to all leaves.
    pub fn leaves(&self) -> &[UTXOLeaf] {
        &self.leaves
    }

    /// Compute the empty leaf hash (used as the sentinel for padding).
    pub fn empty_leaf_hash() -> [u8; 32] {
        UTXOLeaf::empty().hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::pubkey::Pubkey;

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::new(vec![], 3).unwrap();
        assert_eq!(tree.num_leaves(), 8);
        assert_eq!(tree.available_slots().len(), 8);
        assert_eq!(tree.total_amount(), 0);
    }

    #[test]
    fn test_single_leaf() {
        let owner = Pubkey::new_unique();
        let leaf = UTXOLeaf::standard(owner, 1_000_000);
        let leaf_hash = leaf.hash();

        let tree = MerkleTree::new(vec![leaf], 3).unwrap();
        assert_eq!(tree.num_leaves(), 8);

        // Root should be deterministic
        let root = tree.root();
        assert_ne!(root, [0u8; 32]);

        // Verify proof for leaf 0
        let proof = tree.get_proof(0).unwrap();
        assert!(MerkleTree::verify_proof(&leaf_hash, &proof, &root));
    }

    #[test]
    fn test_deterministic_root() {
        let owner = Pubkey::new_unique();
        let leaves = vec![
            UTXOLeaf::standard(owner, 100),
            UTXOLeaf::standard(Pubkey::new_unique(), 200),
        ];

        let tree1 = MerkleTree::new(leaves.clone(), 4).unwrap();
        let tree2 = MerkleTree::new(leaves, 4).unwrap();
        assert_eq!(tree1.root(), tree2.root(), "Same leaves should produce same root");
    }

    #[test]
    fn test_update_changes_root() {
        let owner = Pubkey::new_unique();
        let leaf = UTXOLeaf::standard(owner, 1_000_000);

        let mut tree = MerkleTree::new(vec![leaf], 3).unwrap();
        let root_before = tree.root();

        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);
        let root_after = tree.update_leaf(0, new_leaf).unwrap();

        assert_ne!(root_before, root_after, "Updating a leaf should change the root");
    }

    #[test]
    fn test_proof_roundtrip_after_update() {
        let owner = Pubkey::new_unique();
        let leaves = vec![
            UTXOLeaf::standard(owner, 100),
            UTXOLeaf::standard(Pubkey::new_unique(), 200),
            UTXOLeaf::standard(Pubkey::new_unique(), 300),
        ];

        let mut tree = MerkleTree::new(leaves, 3).unwrap();

        // Update leaf at index 2
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 999);
        tree.update_leaf(2, new_leaf.clone()).unwrap();

        let root = tree.root();
        let leaf_hash = new_leaf.hash();
        let proof = tree.get_proof(2).unwrap();
        assert!(MerkleTree::verify_proof(&leaf_hash, &proof, &root));
    }

    #[test]
    fn test_total_amount_validation() {
        let owner = Pubkey::new_unique();
        let leaves = vec![
            UTXOLeaf::standard(owner, 100),
            UTXOLeaf::standard(Pubkey::new_unique(), 200),
        ];

        let tree = MerkleTree::new(leaves, 3).unwrap();
        assert!(tree.validate_total_amount(300));
        assert!(!tree.validate_total_amount(299));
        assert!(!tree.validate_total_amount(301));
    }

    #[test]
    fn test_available_slots() {
        let owner = Pubkey::new_unique();
        let leaves = vec![
            UTXOLeaf::standard(owner, 100),
            UTXOLeaf::standard(Pubkey::new_unique(), 200),
        ];

        let tree = MerkleTree::new(leaves, 3).unwrap();
        let slots = tree.available_slots();
        // 8 total slots - 2 non-empty = 6 empty
        assert_eq!(slots.len(), 6);
        // First two should be occupied
        assert_eq!(slots[0], 2);
    }

    #[test]
    fn test_too_many_leaves() {
        let leaves: Vec<UTXOLeaf> = (0..5).map(|_| UTXOLeaf::standard(Pubkey::new_unique(), 100)).collect();
        let result = MerkleTree::new(leaves, 2); // max 4 leaves
        assert!(result.is_err());
    }

    #[test]
    fn test_update_out_of_bounds() {
        let tree = MerkleTree::new(vec![], 2).unwrap();
        let mut tree = tree;
        let result = tree.update_leaf(4, UTXOLeaf::empty()); // max index is 3
        assert!(result.is_err());
    }

    #[test]
    fn test_proof_out_of_bounds() {
        let tree = MerkleTree::new(vec![], 2).unwrap();
        let result = tree.get_proof(4);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_proof_fails_wrong_root() {
        let leaf_hash = UTXOLeaf::standard(Pubkey::new_unique(), 100).hash();
        let wrong_root = [99u8; 32];
        assert!(!MerkleTree::verify_proof(&leaf_hash, &[], &wrong_root));
    }

    #[test]
    fn test_all_empty_leaves_same_hash() {
        let h1 = UTXOLeaf::empty().hash();
        let h2 = UTXOLeaf::empty().hash();
        assert_eq!(h1, h2, "Empty leaves should have the same hash");
    }
}
