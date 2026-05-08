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

use ignite_pay_state_channel::merkle::MerkleTree;
use ignite_pay_state_channel::types::UTXOLeaf;
use solana_pubkey::Pubkey;

#[test]
fn test_empty_tree_deterministic_root() {
    let tree1 = MerkleTree::new(vec![], 4).unwrap();
    let tree2 = MerkleTree::new(vec![], 4).unwrap();
    assert_eq!(
        tree1.root(),
        tree2.root(),
        "Two empty trees with same depth should have identical roots"
    );
}

#[test]
fn test_different_depths_different_roots() {
    let tree1 = MerkleTree::new(vec![], 3).unwrap();
    let tree2 = MerkleTree::new(vec![], 4).unwrap();
    assert_ne!(
        tree1.root(),
        tree2.root(),
        "Trees with different depths should have different roots"
    );
}

#[test]
fn test_single_leaf_tree() {
    let owner = Pubkey::new_unique();
    let leaf = UTXOLeaf::standard(owner, 1_000_000);
    let leaf_hash = leaf.hash();

    let mut tree = MerkleTree::new(vec![leaf], 4).unwrap();

    // Proof should verify
    let root = tree.root();
    let proof = tree.get_proof(0).unwrap();
    assert_eq!(proof.len(), 4); // depth = 4
    assert!(MerkleTree::verify_proof(&leaf_hash, &proof, &root));

    // Update and re-verify
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);
    let new_hash = new_leaf.hash();
    tree.update_leaf(0, new_leaf).unwrap();

    let new_root = tree.root();
    assert_ne!(root, new_root);

    let new_proof = tree.get_proof(0).unwrap();
    assert!(MerkleTree::verify_proof(&new_hash, &new_proof, &new_root));

    // Old proof should not verify against new root
    assert!(!MerkleTree::verify_proof(&leaf_hash, &proof, &new_root));
}

#[test]
fn test_multiple_leaves_proofs() {
    let leaves: Vec<UTXOLeaf> = (0..4)
        .map(|i| UTXOLeaf::standard(Pubkey::new_unique(), (i + 1) * 100))
        .collect();

    let tree = MerkleTree::new(leaves.clone(), 3).unwrap();
    let root = tree.root();

    // Verify proof for every leaf
    for (i, leaf) in leaves.iter().enumerate() {
        let proof = tree.get_proof(i).unwrap();
        let leaf_hash = leaf.hash();
        assert!(
            MerkleTree::verify_proof(&leaf_hash, &proof, &root),
            "Proof for leaf {} should verify",
            i
        );
    }
}

#[test]
fn test_update_preserves_other_proofs() {
    let leaves: Vec<UTXOLeaf> = (0..4)
        .map(|i| UTXOLeaf::standard(Pubkey::new_unique(), (i + 1) * 100))
        .collect();

    let mut tree = MerkleTree::new(leaves, 3).unwrap();

    // Update leaf 2
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 999);
    tree.update_leaf(2, new_leaf).unwrap();

    let root = tree.root();

    // Verify proofs for ALL leaves still work
    for i in 0..4 {
        let leaf = tree.get_leaf(i).unwrap();
        let proof = tree.get_proof(i).unwrap();
        let leaf_hash = leaf.hash();
        assert!(
            MerkleTree::verify_proof(&leaf_hash, &proof, &root),
            "Proof for leaf {} should still verify after updating leaf 2",
            i
        );
    }
}

#[test]
fn test_total_amount_after_updates() {
    let leaves: Vec<UTXOLeaf> = (0..3)
        .map(|i| UTXOLeaf::standard(Pubkey::new_unique(), (i + 1) * 1000))
        .collect();

    let mut tree = MerkleTree::new(leaves, 3).unwrap();
    assert!(tree.validate_total_amount(6000));

    // Update leaf 0: transfer from 1000 to 500
    tree.update_leaf(0, UTXOLeaf::standard(Pubkey::new_unique(), 500)).unwrap();
    assert!(tree.validate_total_amount(5500));
    assert!(!tree.validate_total_amount(6000));
}

#[test]
fn test_all_empty_leaves() {
    let tree = MerkleTree::new(vec![], 5).unwrap();
    assert_eq!(tree.num_leaves(), 32);
    assert_eq!(tree.available_slots().len(), 32);
    assert_eq!(tree.total_amount(), 0);
    assert!(tree.validate_total_amount(0));
}

#[test]
fn test_proof_for_empty_leaf() {
    let tree = MerkleTree::new(vec![], 3).unwrap();
    let root = tree.root();
    let empty_hash = UTXOLeaf::empty().hash();

    let proof = tree.get_proof(5).unwrap();
    assert!(MerkleTree::verify_proof(&empty_hash, &proof, &root));
}

#[test]
fn test_tampered_proof_rejected() {
    let leaves: Vec<UTXOLeaf> = (0..3)
        .map(|i| UTXOLeaf::standard(Pubkey::new_unique(), (i + 1) * 100))
        .collect();

    let tree = MerkleTree::new(leaves.clone(), 3).unwrap();
    let root = tree.root();

    let mut proof = tree.get_proof(0).unwrap();
    // Tamper with a proof node
    if !proof.is_empty() {
        proof[0] = [99u8; 32];
    }

    let leaf_hash = leaves[0].hash();
    assert!(!MerkleTree::verify_proof(&leaf_hash, &proof, &root));
}

#[test]
fn test_leaf_hash_deterministic() {
    let leaf1 = UTXOLeaf::standard(Pubkey::new_unique(), 100);
    let leaf2 = leaf1.clone();
    assert_eq!(leaf1.hash(), leaf2.hash());

    // Different amount -> different hash
    let leaf3 = UTXOLeaf::standard(leaf1.owner, 200);
    assert_ne!(leaf1.hash(), leaf3.hash());
}

#[test]
fn test_htlc_leaf_hash_differs_from_standard() {
    let owner = Pubkey::new_unique();
    let standard = UTXOLeaf::standard(owner, 100);
    let htlc = UTXOLeaf::htlc(owner, 100, [42u8; 32], 500, Pubkey::new_unique());
    assert_ne!(standard.hash(), htlc.hash());
}
