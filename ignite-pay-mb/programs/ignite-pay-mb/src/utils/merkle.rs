use solana_sha256_hasher::hashv;

use crate::error::ErrorCode;

/// Verify a Sum Merkle proof.
///
/// Domain separators: 0x00 for leaf nodes, 0x01 for internal nodes.
/// Internal node hash = SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)
/// where lo/hi are sorted by hash lexicographic order.
/// Returns `(root_hash_matches, computed_total_sum)`.
pub fn verify_sum_merkle_proof(
    leaf_hash: &[u8; 32],
    leaf_amount: u64,
    sibling_hashes: &[[u8; 32]],
    sibling_sums: &[u64],
    expected_root: &[u8; 32],
) -> anchor_lang::Result<(bool, u64)> {
    let mut current_hash = *leaf_hash;
    let mut current_sum = leaf_amount;

    for i in 0..sibling_hashes.len() {
        let (lo_hash, lo_sum, hi_hash, hi_sum) = if current_hash <= sibling_hashes[i] {
            (current_hash, current_sum, sibling_hashes[i], sibling_sums[i])
        } else {
            (sibling_hashes[i], sibling_sums[i], current_hash, current_sum)
        };

        // Internal node: SHA256(0x01 || lo_hash || lo_sum_le || hi_hash || hi_sum_le)
        let mut buf = [0u8; 81]; // 1 + 32 + 8 + 32 + 8
        buf[0] = 0x01; // internal node domain separator
        buf[1..33].copy_from_slice(&lo_hash);
        buf[33..41].copy_from_slice(&lo_sum.to_le_bytes());
        buf[41..73].copy_from_slice(&hi_hash);
        buf[73..81].copy_from_slice(&hi_sum.to_le_bytes());

        current_hash = hashv(&[&buf]).to_bytes();
        current_sum = current_sum
            .checked_add(sibling_sums[i])
            .ok_or(ErrorCode::ArithmeticOverflow)?;
    }

    Ok((current_hash == *expected_root, current_sum))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a leaf hash matching the on-chain format:
    /// SHA256(0x00 || data) — simplified for testing the verify function directly.
    fn hash_leaf_test(data: &[u8]) -> [u8; 32] {
        let mut buf = vec![0x00];
        buf.extend_from_slice(data);
        hashv(&[&buf]).to_bytes()
    }

    /// Build an internal node hash matching the on-chain format.
    fn hash_internal_test(lo_hash: &[u8; 32], lo_sum: u64, hi_hash: &[u8; 32], hi_sum: u64) -> [u8; 32] {
        let mut buf = [0u8; 81];
        buf[0] = 0x01;
        buf[1..33].copy_from_slice(lo_hash);
        buf[33..41].copy_from_slice(&lo_sum.to_le_bytes());
        buf[41..73].copy_from_slice(hi_hash);
        buf[73..81].copy_from_slice(&hi_sum.to_le_bytes());
        hashv(&[&buf]).to_bytes()
    }

    #[test]
    fn test_single_leaf_proof() {
        let leaf_hash = hash_leaf_test(b"leaf1");
        let leaf_amount = 100u64;

        // Single leaf: root = leaf, no siblings
        let (matches, total) = verify_sum_merkle_proof(
            &leaf_hash, leaf_amount, &[], &[], &leaf_hash,
        ).unwrap();

        assert!(matches);
        assert_eq!(total, 100);
    }

    #[test]
    fn test_two_leaf_proof() {
        let h1 = hash_leaf_test(b"leaf1");
        let h2 = hash_leaf_test(b"leaf2");
        let sum1 = 300u64;
        let sum2 = 700u64;

        // Sort by hash
        let (lo_h, lo_s, hi_h, hi_s) = if h1 <= h2 {
            (h1, sum1, h2, sum2)
        } else {
            (h2, sum2, h1, sum1)
        };
        let root = hash_internal_test(&lo_h, lo_s, &hi_h, hi_s);

        // Verify leaf 1
        let (matches, total) = verify_sum_merkle_proof(
            &h1, sum1, &[h2], &[sum2], &root,
        ).unwrap();
        assert!(matches);
        assert_eq!(total, sum1 + sum2);

        // Verify leaf 2
        let (matches, total) = verify_sum_merkle_proof(
            &h2, sum2, &[h1], &[sum1], &root,
        ).unwrap();
        assert!(matches);
        assert_eq!(total, sum1 + sum2);
    }

    #[test]
    fn test_wrong_root_fails() {
        let h1 = hash_leaf_test(b"leaf1");
        let wrong_root = hash_leaf_test(b"wrong");

        let (matches, _) = verify_sum_merkle_proof(
            &h1, 100, &[], &[], &wrong_root,
        ).unwrap();
        assert!(!matches);
    }

    #[test]
    fn test_four_leaf_proof() {
        // Build a 4-leaf tree manually
        let h0 = hash_leaf_test(b"l0");
        let h1 = hash_leaf_test(b"l1");
        let h2 = hash_leaf_test(b"l2");
        let h3 = hash_leaf_test(b"l3");
        let s0 = 100u64; let s1 = 200u64; let s2 = 300u64; let s3 = 400u64;

        // Level 1: pair (0,1) and (2,3)
        let (lo01, ls01, hi01, hs01) = if h0 <= h1 { (h0, s0, h1, s1) } else { (h1, s1, h0, s0) };
        let n01 = hash_internal_test(&lo01, ls01, &hi01, hs01);

        let (lo23, ls23, hi23, hs23) = if h2 <= h3 { (h2, s2, h3, s3) } else { (h3, s3, h2, s2) };
        let n23 = hash_internal_test(&lo23, ls23, &hi23, hs23);

        // Root: pair (n01, n23)
        let (lor, lsr, hir, hsr) = if n01 <= n23 { (n01, s0+s1, n23, s2+s3) } else { (n23, s2+s3, n01, s0+s1) };
        let root = hash_internal_test(&lor, lsr, &hir, hsr);

        // Verify leaf 0: siblings are h1 (level 0) and n23 (level 1)
        let (matches, total) = verify_sum_merkle_proof(
            &h0, s0, &[h1, n23], &[s1, s2+s3], &root,
        ).unwrap();
        assert!(matches);
        assert_eq!(total, 1000);
    }

    #[test]
    fn test_wrong_sibling_fails() {
        let h1 = hash_leaf_test(b"leaf1");
        let wrong_sibling = hash_leaf_test(b"wrong");
        let root = hash_leaf_test(b"also_wrong");

        let (matches, _) = verify_sum_merkle_proof(
            &h1, 100, &[wrong_sibling], &[200], &root,
        ).unwrap();
        assert!(!matches);
    }
}
