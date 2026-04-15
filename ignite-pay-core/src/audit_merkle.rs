use sha2::{Digest, Sha256};

/// Standalone SHA-256 Merkle tree for audit logs.
/// Uses sorted-pair hashing: `sha256(min || max)` consistent with the
/// state-channel Merkle implementation.
pub struct AuditMerkleTree {
    root_hash: [u8; 32],
}

impl AuditMerkleTree {
    /// Build a Merkle tree from raw leaf data.
    /// Each leaf is hashed individually, then combined level-by-level.
    pub fn new(leaf_data: &[Vec<u8>]) -> Self {
        let leaves: Vec<[u8; 32]> = leaf_data.iter().map(|d| Self::leaf_hash(d)).collect();

        let root_hash = if leaves.is_empty() {
            // Empty tree: hash of empty input
            let mut hasher = Sha256::new();
            hasher.update([]);
            let result = hasher.finalize();
            let mut out = [0u8; 32];
            out.copy_from_slice(&result);
            out
        } else {
            Self::compute_root(&leaves)
        };

        Self { root_hash }
    }

    /// Returns the Merkle root hash.
    pub fn root(&self) -> [u8; 32] {
        self.root_hash
    }

    /// Compute the hash of a single leaf.
    pub fn leaf_hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Compute root from a list of leaf hashes using sorted-pair hashing.
    fn compute_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        assert!(!leaves.is_empty());

        if leaves.len() == 1 {
            return leaves[0];
        }

        let mut current_level: Vec<[u8; 32]> = leaves.to_vec();

        while current_level.len() > 1 {
            let mut next_level: Vec<[u8; 32]> = Vec::with_capacity((current_level.len() + 1) / 2);

            for pair in current_level.chunks(2) {
                if pair.len() == 2 {
                    let (lo, hi) = if &pair[0] < &pair[1] {
                        (pair[0], pair[1])
                    } else {
                        (pair[1], pair[0])
                    };
                    next_level.push(Self::hash_pair(&lo, &hi));
                } else {
                    // Odd node: promote to next level
                    next_level.push(pair[0]);
                }
            }

            current_level = next_level;
        }

        current_level[0]
    }

    /// `sha256(min || max)` — sorted-pair hash.
    fn hash_pair(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(a);
        hasher.update(b);
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree() {
        let tree = AuditMerkleTree::new(&[]);
        // Root should be SHA-256 of empty input
        let expected = Sha256::digest([]);
        assert_eq!(tree.root(), expected.as_slice());
    }

    #[test]
    fn single_leaf() {
        let data = vec![b"hello".to_vec()];
        let tree = AuditMerkleTree::new(&data);
        assert_eq!(tree.root(), AuditMerkleTree::leaf_hash(b"hello"));
    }

    #[test]
    fn two_leaves() {
        let a = AuditMerkleTree::leaf_hash(b"leaf_a");
        let b = AuditMerkleTree::leaf_hash(b"leaf_b");
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };

        let tree = AuditMerkleTree::new(&[b"leaf_a".to_vec(), b"leaf_b".to_vec()]);

        let mut hasher = Sha256::new();
        hasher.update(lo);
        hasher.update(hi);
        let expected = hasher.finalize();

        assert_eq!(tree.root(), expected.as_slice());
    }

    #[test]
    fn three_leaves() {
        let a = AuditMerkleTree::leaf_hash(b"a");
        let b = AuditMerkleTree::leaf_hash(b"b");
        let c = AuditMerkleTree::leaf_hash(b"c");

        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let mut hasher = Sha256::new();
        hasher.update(lo);
        hasher.update(hi);
        let ab_hash: [u8; 32] = hasher.finalize().into();

        // Level 1: [ab_hash, c] — sorted pair
        let (lo2, hi2) = if ab_hash < c { (ab_hash, c) } else { (c, ab_hash) };
        let mut hasher2 = Sha256::new();
        hasher2.update(lo2);
        hasher2.update(hi2);
        let expected = hasher2.finalize();

        let tree = AuditMerkleTree::new(&[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        assert_eq!(tree.root(), expected.as_slice());
    }

    #[test]
    fn deterministic() {
        let data: Vec<Vec<u8>> = (0..10).map(|i| vec![i]).collect();
        let t1 = AuditMerkleTree::new(&data);
        let t2 = AuditMerkleTree::new(&data);
        assert_eq!(t1.root(), t2.root());
    }

    #[test]
    fn different_data_different_root() {
        let t1 = AuditMerkleTree::new(&[b"foo".to_vec()]);
        let t2 = AuditMerkleTree::new(&[b"bar".to_vec()]);
        assert_ne!(t1.root(), t2.root());
    }
}
