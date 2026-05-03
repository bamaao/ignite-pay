use sha2::{Sha256, Digest};
/// A signed payment voucher (off-chain).
#[derive(Clone, Debug)]
pub struct Voucher {
    pub channel_id: [u8; 32],
    pub seq: u64,
    pub amount: u64,
    pub buyer_pubkey: [u8; 32],
    pub buyer_sig: [u8; 64],
}

/// A node in the sum-Merkle tree.
#[derive(Clone, Debug)]
pub struct SumMerkleNode {
    pub hash: [u8; 32],
    pub sum: u64,
}

/// The full sum-Merkle tree, storing all levels for proof generation.
pub struct MerkleTree {
    /// levels[0] = leaves, levels[last] = root
    levels: Vec<Vec<SumMerkleNode>>,
}

/// A Merkle proof for a single leaf.
pub struct MerkleProof {
    pub sibling_hashes: Vec<[u8; 32]>,
    pub sibling_sums: Vec<u64>,
}

/// Hash a leaf: `SHA256(0x00 || channel || seq_le || amount_le || buyer || sig)`
pub fn hash_leaf(voucher: &Voucher) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&[0x00]);
    hasher.update(&voucher.channel_id);
    hasher.update(&voucher.seq.to_le_bytes());
    hasher.update(&voucher.amount.to_le_bytes());
    hasher.update(&voucher.buyer_pubkey);
    hasher.update(&voucher.buyer_sig);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hasher.finalize());
    result
}

/// Hash an internal node: `SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)`
/// where lo/hi are sorted by hash lexicographic order.
fn hash_internal(lo_hash: &[u8; 32], lo_sum: u64, hi_hash: &[u8; 32], hi_sum: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&[0x01]);
    hasher.update(lo_hash);
    hasher.update(&lo_sum.to_le_bytes());
    hasher.update(hi_hash);
    hasher.update(&hi_sum.to_le_bytes());
    let mut result = [0u8; 32];
    result.copy_from_slice(&hasher.finalize());
    result
}

/// Build a sum-Merkle tree from a list of vouchers.
/// Returns the tree structure from which proofs can be extracted.
pub fn build_sum_merkle_tree(vouchers: &[Voucher]) -> MerkleTree {
    assert!(!vouchers.is_empty(), "need at least one voucher");

    // Build leaf level
    let leaves: Vec<SumMerkleNode> = vouchers
        .iter()
        .map(|v| SumMerkleNode {
            hash: hash_leaf(v),
            sum: v.amount,
        })
        .collect();

    let mut levels = vec![leaves];

    // Build internal levels
    while levels.last().unwrap().len() > 1 {
        let current = levels.last().unwrap();
        let mut next = Vec::new();

        let mut i = 0;
        while i < current.len() {
            if i + 1 < current.len() {
                let (lo_hash, lo_sum, hi_hash, hi_sum) =
                    if current[i].hash <= current[i + 1].hash {
                        (
                            current[i].hash,
                            current[i].sum,
                            current[i + 1].hash,
                            current[i + 1].sum,
                        )
                    } else {
                        (
                            current[i + 1].hash,
                            current[i + 1].sum,
                            current[i].hash,
                            current[i].sum,
                        )
                    };

                next.push(SumMerkleNode {
                    hash: hash_internal(&lo_hash, lo_sum, &hi_hash, hi_sum),
                    sum: lo_sum + hi_sum,
                });
            } else {
                // Odd node: promote to next level
                next.push(current[i].clone());
            }
            i += 2;
        }

        levels.push(next);
    }

    MerkleTree { levels }
}

impl MerkleTree {
    /// Returns the root hash.
    pub fn root_hash(&self) -> [u8; 32] {
        self.levels.last().unwrap()[0].hash
    }

    /// Returns the root sum (total of all leaf amounts).
    pub fn root_sum(&self) -> u64 {
        self.levels.last().unwrap()[0].sum
    }

    /// Generate a Merkle proof for the leaf at `voucher_index`.
    /// Returns sibling hashes and sibling sums along the path from leaf to root.
    pub fn generate_proof(&self, voucher_index: usize) -> MerkleProof {
        let mut sibling_hashes = Vec::new();
        let mut sibling_sums = Vec::new();
        let mut idx = voucher_index;

        for level in &self.levels {
            if level.len() == 1 {
                break; // root level
            }
            let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            if sibling_idx < level.len() {
                sibling_hashes.push(level[sibling_idx].hash);
                sibling_sums.push(level[sibling_idx].sum);
            }
            idx /= 2;
        }

        MerkleProof {
            sibling_hashes,
            sibling_sums,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_voucher(seq: u64, amount: u64) -> Voucher {
        Voucher {
            channel_id: [0x01; 32],
            seq,
            amount,
            buyer_pubkey: [0x02; 32],
            buyer_sig: [seq as u8; 64],
        }
    }

    #[test]
    fn test_single_leaf_tree() {
        let v = make_voucher(0, 1000);
        let tree = build_sum_merkle_tree(&[v.clone()]);

        // Root hash should equal the leaf hash
        let leaf_hash = hash_leaf(&v);
        assert_eq!(tree.root_hash(), leaf_hash);
        assert_eq!(tree.root_sum(), 1000);

        // Proof for single leaf should have no siblings
        let proof = tree.generate_proof(0);
        assert!(proof.sibling_hashes.is_empty());
        assert!(proof.sibling_sums.is_empty());
    }

    #[test]
    fn test_two_leaf_tree() {
        let v1 = make_voucher(0, 300);
        let v2 = make_voucher(1, 700);
        let tree = build_sum_merkle_tree(&[v1.clone(), v2.clone()]);

        assert_eq!(tree.root_sum(), 1000);

        // Root hash != either leaf hash
        let h1 = hash_leaf(&v1);
        let h2 = hash_leaf(&v2);
        assert_ne!(tree.root_hash(), h1);
        assert_ne!(tree.root_hash(), h2);

        // Proofs should each have exactly 1 sibling
        let proof0 = tree.generate_proof(0);
        assert_eq!(proof0.sibling_hashes.len(), 1);
        assert_eq!(proof0.sibling_sums.len(), 1);

        let proof1 = tree.generate_proof(1);
        assert_eq!(proof1.sibling_hashes.len(), 1);
        assert_eq!(proof1.sibling_sums.len(), 1);

        // Verify proof0's sibling is leaf 2
        assert_eq!(proof0.sibling_hashes[0], h2);
        assert_eq!(proof0.sibling_sums[0], 700);
    }

    #[test]
    fn test_four_leaf_tree() {
        let vouchers: Vec<Voucher> = (0..4).map(|i| make_voucher(i, (i + 1) * 100)).collect();
        let tree = build_sum_merkle_tree(&vouchers);

        assert_eq!(tree.root_sum(), 100 + 200 + 300 + 400); // 1000

        // Each proof should have 2 siblings (height = log2(4) = 2)
        let proof0 = tree.generate_proof(0);
        assert_eq!(proof0.sibling_hashes.len(), 2);
    }

    #[test]
    fn test_three_leaf_tree_odd_count() {
        let v1 = make_voucher(0, 100);
        let v2 = make_voucher(1, 200);
        let v3 = make_voucher(2, 300);
        let tree = build_sum_merkle_tree(&[v1, v2, v3]);

        assert_eq!(tree.root_sum(), 600);
    }

    #[test]
    fn test_deterministic_hash() {
        let v = make_voucher(42, 999);
        let h1 = hash_leaf(&v);
        let h2 = hash_leaf(&v);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_vouchers_different_hashes() {
        let v1 = make_voucher(0, 100);
        let v2 = make_voucher(1, 100);
        assert_ne!(hash_leaf(&v1), hash_leaf(&v2));
    }

    #[test]
    #[should_panic(expected = "need at least one voucher")]
    fn test_empty_vouchers_panics() {
        build_sum_merkle_tree(&[]);
    }
}
