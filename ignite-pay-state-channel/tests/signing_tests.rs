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

use ignite_pay_state_channel::signing::{
    leaf_update_message, sign_leaf_update, sign_state, state_message,
    verify_leaf_update_signature, verify_state_signature,
    generate_keypair, to_pubkey,
};
use ignite_pay_state_channel::types::UTXOLeaf;
use solana_pubkey::Pubkey;

#[test]
fn test_leaf_update_full_roundtrip() {
    let signer = generate_keypair();
    let channel_id = [1u8; 32];

    let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 1000);
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500);

    let update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);

    assert!(verify_leaf_update_signature(&update, &to_pubkey(&signer)));
    assert_eq!(update.channel_id, channel_id);
    assert_eq!(update.sequence, 1);
    assert_eq!(update.leaf_index, 0);
    assert_eq!(update.prev_leaf_hash, prev_leaf.hash());
}

#[test]
fn test_leaf_update_tampered_leaf() {
    let signer = generate_keypair();
    let channel_id = [1u8; 32];

    let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 1000);
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500);

    let mut update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
    update.new_leaf.amount = 999;
    assert!(!verify_leaf_update_signature(&update, &to_pubkey(&signer)));
}

#[test]
fn test_leaf_update_tampered_sequence() {
    let signer = generate_keypair();
    let channel_id = [1u8; 32];

    let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 1000);
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500);

    let mut update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
    update.sequence = 2;
    assert!(!verify_leaf_update_signature(&update, &to_pubkey(&signer)));
}

#[test]
fn test_leaf_update_tampered_prev_hash() {
    let signer = generate_keypair();
    let channel_id = [1u8; 32];

    let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 1000);
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500);

    let mut update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
    update.prev_leaf_hash = [99u8; 32];
    assert!(!verify_leaf_update_signature(&update, &to_pubkey(&signer)));
}

#[test]
fn test_leaf_update_wrong_signer() {
    let signer = generate_keypair();
    let wrong_key = generate_keypair();
    let channel_id = [1u8; 32];

    let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 1000);
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500);

    let update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
    assert!(!verify_leaf_update_signature(&update, &to_pubkey(&wrong_key)));
}

#[test]
fn test_state_full_roundtrip() {
    let keypair = generate_keypair();
    let channel_id = [7u8; 32];
    let root = [42u8; 32];

    let sig = sign_state(&channel_id, 10, &root, &keypair);
    assert!(verify_state_signature(&channel_id, 10, &root, &sig, &to_pubkey(&keypair)));
}

#[test]
fn test_state_wrong_channel_id() {
    let keypair = generate_keypair();
    let channel_id = [7u8; 32];
    let root = [42u8; 32];

    let sig = sign_state(&channel_id, 10, &root, &keypair);
    assert!(!verify_state_signature(&[8u8; 32], 10, &root, &sig, &to_pubkey(&keypair)));
}

#[test]
fn test_state_wrong_root() {
    let keypair = generate_keypair();
    let channel_id = [7u8; 32];
    let root = [42u8; 32];

    let sig = sign_state(&channel_id, 10, &root, &keypair);
    assert!(!verify_state_signature(&channel_id, 10, &[43u8; 32], &sig, &to_pubkey(&keypair)));
}

#[test]
fn test_state_wrong_signer() {
    let keypair = generate_keypair();
    let other = generate_keypair();
    let channel_id = [7u8; 32];
    let root = [42u8; 32];

    let sig = sign_state(&channel_id, 10, &root, &keypair);
    assert!(!verify_state_signature(&channel_id, 10, &root, &sig, &to_pubkey(&other)));
}

#[test]
fn test_message_determinism() {
    let cid = [1u8; 32];
    let m1 = state_message(&cid, 5, &[2u8; 32]);
    let m2 = state_message(&cid, 5, &[2u8; 32]);
    assert_eq!(m1, m2);

    let prev = [3u8; 32];
    let new_h = [4u8; 32];
    let lm1 = leaf_update_message(&cid, 1, 0, &prev, &new_h);
    let lm2 = leaf_update_message(&cid, 1, 0, &prev, &new_h);
    assert_eq!(lm1, lm2);
}

#[test]
fn test_different_leaves_different_messages() {
    let cid = [1u8; 32];
    let prev = UTXOLeaf::standard(Pubkey::new_unique(), 100);
    let new1 = UTXOLeaf::standard(Pubkey::new_unique(), 50);
    let new2 = UTXOLeaf::standard(Pubkey::new_unique(), 75);

    let m1 = leaf_update_message(&cid, 1, 0, &prev.hash(), &new1.hash());
    let m2 = leaf_update_message(&cid, 1, 0, &prev.hash(), &new2.hash());
    assert_ne!(m1, m2);
}
