use ed25519_dalek::Signer;
use ignite_pay_state_channel::channel::ChannelManager;
use ignite_pay_state_channel::helpers::{merge_spent_leaves, split_from_rest};
use ignite_pay_state_channel::htlc::HtlcManager;
use ignite_pay_state_channel::merkle::MerkleTree;
use ignite_pay_state_channel::pipeline::Pipeline;
use ignite_pay_state_channel::signing::{
    generate_keypair, sign_leaf_update, sign_state, state_message, claim_message, to_pubkey,
    verify_leaf_update_signature, verify_state_signature,
};
use ignite_pay_state_channel::types::{ChannelStatus, LeafType, SignedState, UTXOLeaf};
use solana_program::hash::hash as solana_hash;
use solana_pubkey::Pubkey;

fn temp_db() -> sled::Db {
    let dir = tempfile::tempdir().unwrap();
    sled::open(dir.path()).unwrap()
}

fn default_vaults() -> (Pubkey, Pubkey) {
    (Pubkey::new_unique(), Pubkey::new_unique())
}

/// Full lifecycle test: open -> split tree -> batch payments -> HTLC create/resolve -> close
#[test]
fn test_full_lifecycle() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let mint = Pubkey::new_unique();
    let deposit = 10_000_000;
    let (vault_a, vault_b) = default_vaults();

    // Step 1: Open channel
    let mut state = mgr
        .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &mint, deposit, 4, 100, &vault_a, &vault_b, 500, 50, None)
        .unwrap();

    assert_eq!(state.metadata.status, ChannelStatus::Open);
    assert_eq!(state.metadata.sequence, 0);
    assert!(state.tree.validate_total_amount(deposit));

    // Step 2: Construct split tree
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 1_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 4_000_000), // rest
    ];

    let signed = mgr
        .construct_split_tree(&mut state, leaves, &user, &provider)
        .unwrap();

    assert_eq!(signed.sequence, 1);
    assert_eq!(state.metadata.sequence, 1);
    assert!(state.tree.validate_total_amount(deposit));

    // Verify dual signatures
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

    // Step 3: Batch payments via pipeline
    let merchant1 = Pubkey::new_unique();
    let merchant2 = Pubkey::new_unique();
    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;

    let mut tree = state.tree;
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);

        // Transfer leaf 0 to merchant1
        pipeline.transfer_leaf(0, merchant1).unwrap();

        // Partial transfer from leaf 1 to empty slot 4
        pipeline.partial_transfer(1, 4, 500_000, merchant2).unwrap();

        let (updates, final_seq) = pipeline.build();
        assert_eq!(updates.len(), 3); // 1 transfer + 2 partial (dest creation + src deduction)
        assert_eq!(final_seq, seq + 4);

        // Verify all updates individually
        for update in &updates {
            assert!(verify_leaf_update_signature(update, &to_pubkey(&user)));
        }

        // Amount conservation
        assert_eq!(tree.total_amount(), deposit);

        // Verify leaf states
        assert_eq!(tree.get_leaf(0).unwrap().owner, merchant1);
        assert_eq!(tree.get_leaf(0).unwrap().amount, 1_000_000);
        assert_eq!(tree.get_leaf(1).unwrap().amount, 1_500_000);
        assert_eq!(tree.get_leaf(4).unwrap().owner, merchant2);
        assert_eq!(tree.get_leaf(4).unwrap().amount, 500_000);
    }

    // Step 4: HTLC create and resolve
    let mut htlc_mgr = HtlcManager::new();
    let beneficiary = Pubkey::new_unique();
    let (hash_lock, preimage) = htlc_mgr.create_htlc(
        1_000_000,
        2,
        to_pubkey(&user),
        beneficiary,
        100,
        200,
    );

    // Apply HTLC creation through pipeline
    let seq_after_batch = seq + 4;
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq_after_batch + 1, &user);
        pipeline.create_htlc(2, hash_lock, 3000, beneficiary, 100, 500).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 1);

        assert_eq!(tree.get_leaf(2).unwrap().leaf_type, LeafType::HTLC);
        assert_eq!(tree.get_leaf(2).unwrap().hash_lock, Some(hash_lock));
        assert_eq!(tree.get_leaf(2).unwrap().beneficiary, Some(beneficiary));
    }

    // Reveal preimage
    htlc_mgr.reveal_preimage(&hash_lock, &preimage).unwrap();

    // Resolve HTLC — BUG-6 fix: pass preimage for verification
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq_after_batch + 2, &user);
        pipeline.resolve_htlc(2, &preimage).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 1);

        let resolved = tree.get_leaf(2).unwrap();
        assert_eq!(resolved.leaf_type, LeafType::Standard);
        assert_eq!(resolved.owner, beneficiary);
        assert_eq!(resolved.amount, 3_000_000);

        // Final amount conservation check
        assert_eq!(tree.total_amount(), deposit);
    }
}

/// Test HTLC timeout and refund flow.
#[test]
fn test_htlc_timeout_refund() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 5_000_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
        .unwrap();

    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    let mut htlc_mgr = HtlcManager::new();
    let beneficiary = Pubkey::new_unique();
    let (hash_lock, _preimage) = htlc_mgr.create_htlc(
        2_000_000,
        0,
        to_pubkey(&user),
        beneficiary,
        100,
        50, // timelock at slot 150
    );

    // Create HTLC on leaf 0
    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        pipeline.create_htlc(0, hash_lock, 2000, beneficiary, 100, 500).unwrap();
        pipeline.build();
    }

    // Check expiry (timelock is 2000, so expire at 2100)
    let expired = htlc_mgr.check_expiry(2100);
    assert_eq!(expired.len(), 1);

    // Refund — BUG-6 fix: pass current_slot for timelock verification
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 2, &user);
        pipeline.refund_htlc(0, 2100).unwrap();
        pipeline.build();

        let refunded = tree.get_leaf(0).unwrap();
        assert_eq!(refunded.owner, to_pubkey(&user));
        assert_eq!(refunded.amount, 2_000_000);
        assert_eq!(tree.total_amount(), 5_000_000);
    }
}

/// Test split_from_rest and merge_spent_leaves helpers.
#[test]
fn test_split_and_merge_helpers() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 10_000_000, 4, 100, &vault_a, &vault_b, 500, 50, None)
        .unwrap();

    // Split into: 2M, 3M, 5M (rest)
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 5_000_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    // Split 1.5M from rest (index 2) into empty slot (index 3)
    let (updates, new_seq) = split_from_rest(
        &channel_id,
        2,
        3,
        1_500_000,
        seq + 1,
        &mut tree,
        &user,
    )
    .unwrap();

    assert_eq!(updates.len(), 2);
    assert_eq!(tree.get_leaf(2).unwrap().amount, 3_500_000);
    assert_eq!(tree.get_leaf(3).unwrap().amount, 1_500_000);
    assert_eq!(tree.total_amount(), 10_000_000);

    // Merge leaves 0 and 1 into leaf 2
    let (updates, _) = merge_spent_leaves(
        &channel_id,
        &[0, 1],
        2,
        new_seq,
        &mut tree,
        &user,
    )
    .unwrap();

    assert_eq!(updates.len(), 3); // clear 0, clear 1, update 2
    assert!(tree.get_leaf(0).unwrap().is_empty());
    assert!(tree.get_leaf(1).unwrap().is_empty());
    // 3_500_000 + 2_000_000 + 3_000_000 = 8_500_000
    assert_eq!(tree.get_leaf(2).unwrap().amount, 8_500_000);
    assert_eq!(tree.total_amount(), 10_000_000);
}

/// Test persistence across restart.
#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let channel_id = {
        let db = sled::open(dir.path()).unwrap();
        let mgr = ChannelManager::new(db).unwrap();

        let user = generate_keypair();
        let provider = generate_keypair();
        let (vault_a, vault_b) = default_vaults();

        let state = mgr
            .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
            .unwrap();

        state.metadata.channel_id
    };

    // Reopen
    let db = sled::open(dir.path()).unwrap();
    let mgr = ChannelManager::new(db).unwrap();
    let loaded = mgr.load_state(&channel_id).unwrap();

    assert_eq!(loaded.metadata.channel_id, channel_id);
    assert_eq!(loaded.metadata.total_deposited, 1_000_000);
    assert!(loaded.tree.validate_total_amount(1_000_000));
}

/// Test all UTXOs spent scenario.
#[test]
fn test_all_utxos_spent() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 2, 100, &vault_a, &vault_b, 500, 50, None)
        .unwrap();

    // Split into all 4 slots (all owned by user)
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 250_000),
        UTXOLeaf::standard(to_pubkey(&user), 250_000),
        UTXOLeaf::standard(to_pubkey(&user), 250_000),
        UTXOLeaf::standard(to_pubkey(&user), 250_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // No available slots
    assert_eq!(state.tree.available_slots().len(), 0);
    assert_eq!(state.tree.total_amount(), 1_000_000);
}

/// Test close channel flow.
#[test]
fn test_close_channel_flow() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(&to_pubkey(&user), &to_pubkey(&provider), &Pubkey::new_unique(), 1_000_000, 3, 100, &vault_a, &vault_b, 500, 50, None)
        .unwrap();

    assert_eq!(state.metadata.status, ChannelStatus::Open);

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
    // BUG-3 fix: transitions to Settling, not directly to Closed
    assert_eq!(state.metadata.status, ChannelStatus::Settling);
    assert_eq!(state.metadata.settle_deadline, Some(300));

    // Verify persisted
    let loaded = mgr.load_state(&state.metadata.channel_id).unwrap();
    assert_eq!(loaded.metadata.status, ChannelStatus::Settling);
}

// ============================================================================
// TEST-1: Full lifecycle integration test with HTLC (Open → Split → Transfer
//         → HTLC(Create → Resolve) → Close → Claim → Finalize)
// ============================================================================

/// TEST-1: End-to-end integration test covering the full payment lifecycle
/// with HTLC, dispute, and settlement paths.
#[test]
fn test_full_lifecycle_with_htlc_and_settlement() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let deposit = 10_000_000;
    let (vault_a, vault_b) = default_vaults();

    // --- Phase 1: Open channel ---
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            deposit,
            4, // depth 4 = 16 slots
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    assert_eq!(state.metadata.status, ChannelStatus::Open);
    assert!(state.tree.validate_total_amount(deposit));

    // --- Phase 2: Split tree ---
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 5_000_000),
    ];
    let signed_split = mgr
        .construct_split_tree(&mut state, leaves, &user, &provider)
        .unwrap();
    assert_eq!(signed_split.sequence, 1);

    // --- Phase 3: Transfer via pipeline ---
    let merchant = Pubkey::new_unique();
    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        pipeline.transfer_leaf(0, merchant).unwrap();
        let (updates, final_seq) = pipeline.build();
        assert_eq!(updates.len(), 1);
        assert_eq!(final_seq, seq + 2);
        assert_eq!(tree.get_leaf(0).unwrap().owner, merchant);
        assert_eq!(tree.total_amount(), deposit);
    }

    // --- Phase 4: Create HTLC ---
    let beneficiary = Pubkey::new_unique();
    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 2, &user);
        // FLOW-6: timelock must be > current_slot + challenge_duration + SAFETY_MARGIN
        // 2000 > 100 + 500 + 1000 = 1600 ✓
        pipeline
            .create_htlc(1, hash_lock, 2000, beneficiary, 100, 500)
            .unwrap();
        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 1);
        assert_eq!(tree.get_leaf(1).unwrap().leaf_type, LeafType::HTLC);
        assert_eq!(tree.get_leaf(1).unwrap().amount, 3_000_000);
    }

    // --- Phase 5: Resolve HTLC with correct preimage ---
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 3, &user);
        pipeline.resolve_htlc(1, &preimage).unwrap();
        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 1);
        assert_eq!(tree.get_leaf(1).unwrap().leaf_type, LeafType::Standard);
        assert_eq!(tree.get_leaf(1).unwrap().owner, beneficiary);
        assert_eq!(tree.total_amount(), deposit);
    }

    // --- Phase 6: Cooperative close ---
    // Reconstruct state from tree for close
    let new_root = tree.root();
    let new_seq = seq + 3;
    state.tree = tree;
    state.metadata.sequence = new_seq;
    state.metadata.current_root = new_root;

    let signed_close = SignedState {
        channel_id,
        sequence: new_seq,
        root: new_root,
        sig_a: sign_state(&channel_id, new_seq, &new_root, &user),
        sig_b: sign_state(&channel_id, new_seq, &new_root, &provider),
    };

    mgr.close_channel(
        &mut state,
        &signed_close,
        &to_pubkey(&user),
        &to_pubkey(&provider),
        500,
        200,
    )
    .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);
    assert_eq!(state.metadata.settle_deadline, Some(700));

    // --- Phase 7: Claim leaves ---
    // Leaf 0 is owned by merchant (not a channel participant), so claim leaf 1 (beneficiary)
    // and leaf 2 (user). But beneficiary is not a participant either. Only user/provider can claim.
    // Leaf 2 is owned by user, leaf 0 by merchant (neither user nor provider can claim it).
    // Let's claim leaf 2 (user's 5M).
    let claim_msg = claim_message(&channel_id, 2, 5_000_000, 600);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    mgr.claim_leaf(
        &mut state,
        2,
        5_000_000,
        &to_pubkey(&user),
        600,
        &claim_sig,
    )
    .unwrap();
    assert_eq!(state.metadata.total_claimed, 5_000_000);

    // --- Phase 8: Finalize settlement ---
    let fin_msg = state_message(&channel_id, 700, &state.metadata.current_root);
    let fin_sig = user.sign(&fin_msg).to_bytes();
    let (refund_a, refund_b) = mgr
        .finalize_settlement(&mut state, 700, &to_pubkey(&user), &fin_sig)
        .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Closed);
    // Unclaimed = 10M - 5M = 5M, all goes to user (deposit_a = 10M, deposit_b = 0)
    assert_eq!(refund_a, 5_000_000);
    assert_eq!(refund_b, 0);
}

// ============================================================================
// TEST-2: HtlcManager persistence recovery test
// ============================================================================

/// TEST-2: Verify HtlcManager state survives drop and reload via sled.
#[test]
fn test_htlc_manager_persistence_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let channel_id = [77u8; 32];

    let (hash_lock1, preimage1, hash_lock2) = {
        let db = sled::open(dir.path()).unwrap();
        let mut mgr = HtlcManager::with_db(db, channel_id);

        let (hl1, pi1) = mgr.create_htlc(
            100_000,
            0,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            100,
            200,
        );
        let (hl2, _) = mgr.create_htlc(
            200_000,
            1,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            100,
            400,
        );

        // Reveal first HTLC
        mgr.reveal_preimage(&hl1, &pi1).unwrap();

        (hl1, pi1, hl2)
    };

    // Reload from DB
    {
        let db = sled::open(dir.path()).unwrap();
        let mgr = HtlcManager::with_db(db, channel_id);

        // Verify records restored
        let rec1 = mgr.get_record(&hash_lock1).unwrap();
        assert_eq!(rec1.amount, 100_000);
        assert_eq!(rec1.state, ignite_pay_state_channel::htlc::HtlcState::Revealed);
        assert_eq!(rec1.preimage, preimage1);

        let rec2 = mgr.get_record(&hash_lock2).unwrap();
        assert_eq!(rec2.amount, 200_000);
        assert_eq!(rec2.state, ignite_pay_state_channel::htlc::HtlcState::Pending);
    }
}

// ============================================================================
// TEST-3: Pipeline → apply_leaf_update_batch cross-module test
// ============================================================================

/// TEST-3: Verify that Pipeline output is accepted by ChannelManager::apply_leaf_update_batch.
#[test]
fn test_pipeline_to_batch_cross_module() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            3,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split tree
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 400_000),
        UTXOLeaf::standard(to_pubkey(&user), 600_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider)
        .unwrap();

    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;

    // Create pipeline and produce updates
    let mut tree = state.tree;
    let recipient = Pubkey::new_unique();
    let (updates, final_seq) = {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        pipeline.transfer_leaf(0, recipient).unwrap();
        pipeline.partial_transfer(1, 2, 200_000, Pubkey::new_unique()).unwrap();
        pipeline.build()
    };
    assert_eq!(updates.len(), 3); // 1 transfer + 2 partial transfer ops

    // Now sync state back — re-load state and apply updates via batch
    state.tree = tree;
    state.metadata.sequence = seq;
    state.metadata.current_root = state.tree.root();

    // The pipeline already modified the tree, so to test apply_leaf_update_batch
    // we need a fresh state. Reload from DB.
    let mut fresh_state = mgr.load_state(&channel_id).unwrap();

    // Apply the pipeline's updates via the batch method
    mgr.apply_leaf_update_batch(&mut fresh_state, &updates, &to_pubkey(&user))
        .unwrap();

    assert_eq!(fresh_state.metadata.sequence, final_seq - 1);
    assert_eq!(fresh_state.tree.get_leaf(0).unwrap().owner, recipient);
    assert_eq!(fresh_state.tree.get_leaf(0).unwrap().amount, 400_000);
    assert_eq!(fresh_state.tree.get_leaf(1).unwrap().amount, 400_000);
    assert_eq!(fresh_state.tree.get_leaf(2).unwrap().amount, 200_000);
    assert_eq!(fresh_state.tree.total_amount(), 1_000_000);

    // Verify persisted state matches
    let loaded = mgr.load_state(&channel_id).unwrap();
    assert_eq!(loaded.metadata.sequence, fresh_state.metadata.sequence);
    assert_eq!(loaded.tree.root(), fresh_state.tree.root());
}

// ============================================================================
// TEST-4: Security boundary tests
// ============================================================================

/// TEST-4a: tree_depth = 0 should work (1 slot) or be rejected.
#[test]
fn test_tree_depth_zero() {
    let result = MerkleTree::new(vec![UTXOLeaf::standard(Pubkey::new_unique(), 100)], 0);
    // depth 0 means 2^0 = 1 leaf slot, so one leaf should fit
    assert!(result.is_ok());
    let tree = result.unwrap();
    assert_eq!(tree.num_leaves(), 1);
}

/// TEST-4b: tree_depth = 0 with too many leaves should fail.
#[test]
fn test_tree_depth_zero_too_many_leaves() {
    let leaves = vec![
        UTXOLeaf::standard(Pubkey::new_unique(), 50),
        UTXOLeaf::standard(Pubkey::new_unique(), 50),
    ];
    let result = MerkleTree::new(leaves, 0);
    assert!(result.is_err());
}

/// TEST-4c: Sequence at u64::MAX should not panic (saturating behavior).
#[test]
fn test_sequence_u64_max_no_panic() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            3,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Force sequence to near max
    state.metadata.sequence = u64::MAX - 1;

    // Apply a leaf update with sequence u64::MAX
    let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        u64::MAX,
        0,
        &prev_leaf,
        new_leaf,
        &user,
    );

    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();
    assert_eq!(state.metadata.sequence, u64::MAX);

    // Pipeline with sequence at u64::MAX should not panic (saturating increment)
    let mut tree = state.tree;
    let mut pipeline = Pipeline::new(&mut tree, state.metadata.channel_id, u64::MAX, &user);
    // Attempt a transfer — pipeline uses u64::MAX as the sequence, then saturates
    let result = pipeline.transfer_leaf(0, Pubkey::new_unique());
    assert!(result.is_ok());
}

/// TEST-4d: Amount u64 overflow in total_amount sum should saturate, not panic.
#[test]
fn test_amount_overflow_protection() {
    // Create leaves whose sum overflows u64
    let leaves = vec![
        UTXOLeaf::standard(Pubkey::new_unique(), u64::MAX),
        UTXOLeaf::standard(Pubkey::new_unique(), 1),
    ];
    let tree = MerkleTree::new(leaves, 2).unwrap();
    // total_amount uses saturating_add — should not panic, caps at u64::MAX
    let total = tree.total_amount();
    assert_eq!(total, u64::MAX, "saturating_add caps at u64::MAX");
    // validate_total_amount should detect this is wrong
    assert!(!tree.validate_total_amount(0)); // clearly not 0
}

/// TEST-4e: Replay attack — applying same update twice should fail.
#[test]
fn test_replay_attack_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let signer = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&signer),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            3,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Create and apply update
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

    // First application succeeds
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&signer))
        .unwrap();
    assert_eq!(state.metadata.sequence, 1);

    // Replay same update — should fail (sequence mismatch: expected 2, got 1)
    let result = mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&signer));
    assert!(result.is_err());
}

// ============================================================================
// TEST-5: Merkle Proof on-chain compatibility (sorted-pair hashv)
// ============================================================================

/// TEST-5: Verify that Merkle proofs follow the Solana on-chain convention
/// using hashv(&[min, max]) sorted-pair hashing.
///
/// This tests the off-chain proof generation against the same algorithm
/// used by Solana's `compression.rs:verify_proof_locally`.
#[test]
fn test_merkle_proof_on_chain_compatible() {
    // Build a tree with specific leaves
    let leaves = vec![
        UTXOLeaf::standard(Pubkey::new_unique(), 100),
        UTXOLeaf::standard(Pubkey::new_unique(), 200),
        UTXOLeaf::standard(Pubkey::new_unique(), 300),
    ];
    let tree = MerkleTree::new(leaves, 3).unwrap(); // 8 slots

    // For each non-empty leaf, generate proof and verify
    for i in 0..3 {
        let leaf = tree.get_leaf(i).unwrap();
        let leaf_hash = leaf.hash();
        let proof = tree.get_proof(i).unwrap();
        let root = tree.root();

        // Self-verify (off-chain)
        assert!(
            MerkleTree::verify_proof(&leaf_hash, &proof, &root),
            "Proof for leaf {} should verify",
            i
        );

        // Verify the proof follows sorted-pair hashing (hashv pattern)
        // by manually recomputing the path
        let mut current = leaf_hash;
        for sibling in &proof {
            let pair = if current < *sibling {
                vec![current.as_ref(), sibling.as_ref()]
            } else {
                vec![sibling.as_ref(), current.as_ref()]
            };
            current = solana_program::hash::hashv(&pair).to_bytes();
        }
        assert_eq!(
            current, root,
            "Manual sorted-pair hashv verification failed for leaf {}",
            i
        );
    }

    // Verify tampered proof fails
    let leaf_hash = tree.get_leaf(0).unwrap().hash();
    let mut proof = tree.get_proof(0).unwrap();
    if !proof.is_empty() {
        proof[0] = [0u8; 32]; // Tamper with first sibling
        assert!(!MerkleTree::verify_proof(&leaf_hash, &proof, &tree.root()));
    }
}

/// TEST-5b: Proof verification after leaf update still works.
#[test]
fn test_proof_after_update_on_chain_compatible() {
    let leaves = vec![
        UTXOLeaf::standard(Pubkey::new_unique(), 500),
        UTXOLeaf::empty(),
    ];
    let mut tree = MerkleTree::new(leaves.clone(), 2).unwrap();

    // Update leaf 1
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 300);
    tree.update_leaf(1, new_leaf.clone()).unwrap();

    // Verify proof for updated leaf
    let leaf_hash = new_leaf.hash();
    let proof = tree.get_proof(1).unwrap();
    let root = tree.root();
    assert!(MerkleTree::verify_proof(&leaf_hash, &proof, &root));

    // Verify with sorted-pair hashv
    let mut current = leaf_hash;
    for sibling in &proof {
        let pair = if current < *sibling {
            vec![current.as_ref(), sibling.as_ref()]
        } else {
            vec![sibling.as_ref(), current.as_ref()]
        };
        current = solana_program::hash::hashv(&pair).to_bytes();
    }
    assert_eq!(current, root);
}

// ============================================================================
// AUDIT TEST-1: Dual-funded channel → construct_split_tree integration
// ============================================================================

/// TEST-1: End-to-end flow: open → fund_channel → construct_split_tree
/// Design doc §10.6.2: dual-funded channel lifecycle.
#[test]
fn test_fund_channel_then_split_tree() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    // Open with user deposit
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Provider funds with 500_000
    let update = mgr.fund_channel(&mut state, &provider, 500_000, None).unwrap();
    assert_eq!(state.metadata.deposit_b, 500_000);
    assert_eq!(state.metadata.total_deposited, 1_500_000);
    assert_eq!(state.metadata.sequence, 1);

    // Verify update is a valid LeafUpdate
    assert_eq!(update.channel_id, state.metadata.channel_id);
    assert_eq!(update.sequence, 1);

    // Split tree with both user and provider leaves
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 400_000),
        UTXOLeaf::standard(to_pubkey(&user), 600_000),
        UTXOLeaf::standard(to_pubkey(&provider), 500_000),
    ];

    let signed = mgr
        .construct_split_tree(&mut state, leaves, &user, &provider)
        .unwrap();

    assert_eq!(signed.sequence, 2);
    assert_eq!(state.metadata.sequence, 2);
    assert!(state.tree.validate_total_amount(1_500_000));

    // Verify both parties have correct amounts
    let user_total: u64 = state
        .tree
        .leaves()
        .iter()
        .filter(|l| l.owner == to_pubkey(&user))
        .map(|l| l.amount)
        .sum();
    let provider_total: u64 = state
        .tree
        .leaves()
        .iter()
        .filter(|l| l.owner == to_pubkey(&provider))
        .map(|l| l.amount)
        .sum();
    assert_eq!(user_total, 1_000_000);
    assert_eq!(provider_total, 500_000);
}

// ============================================================================
// AUDIT TEST-4: VerifyHTLC and HTLCRefund in Challenged status
// ============================================================================

/// TEST-4: VerifyHTLC works in Challenged status (not just Settling).
#[test]
fn test_verify_htlc_in_challenged_status() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    // Open and split
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    let beneficiary = to_pubkey(&provider);
    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    // Create HTLC leaf at index 1
    let prev_leaf = state.tree.get_leaf(1).unwrap().clone();
    let htlc_leaf = UTXOLeaf::htlc(
        to_pubkey(&user),
        500_000,
        hash_lock,
        5000, // timelock far in the future
        beneficiary,
    );
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        1,
        &prev_leaf,
        htlc_leaf,
        &user,
    );
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();

    // Trigger challenge (move to Challenged status)
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    // BUG-23: sign with submitted_sequence, not current_slot
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Challenged);

    // Set settle_deadline manually (normally set during settle_after_timeout)
    state.metadata.settle_deadline = Some(1000);

    // Provider (beneficiary) can verify HTLC in Challenged status
    let claim_msg = claim_message(&state.metadata.channel_id, 1, 500_000, 300);
    let claim_sig = provider.sign(&claim_msg).to_bytes();
    mgr.claim_htlc_verify(
        &mut state,
        1,
        &preimage,
        &beneficiary,
        300,
        &claim_sig,
    )
    .unwrap();

    assert_eq!(state.metadata.total_claimed, 500_000);
    assert!(state.metadata.claimed_leaves.contains(&1));
}

/// TEST-4b: HTLCRefund works in Challenged status.
#[test]
fn test_htlc_refund_in_challenged_status() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    // Create HTLC leaf at index 1 with timelock = 300
    let prev_leaf = state.tree.get_leaf(1).unwrap().clone();
    let htlc_leaf = UTXOLeaf::htlc(
        to_pubkey(&user),
        500_000,
        hash_lock,
        300, // timelock at slot 300
        to_pubkey(&provider),
    );
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        1,
        &prev_leaf,
        htlc_leaf,
        &user,
    );
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();

    // Trigger challenge
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    // BUG-23: sign with submitted_sequence, not current_slot
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    state.metadata.settle_deadline = Some(1000);

    // User can refund HTLC after timelock expires (slot > 300)
    let refund_msg = claim_message(&state.metadata.channel_id, 1, 500_000, 400);
    let refund_sig = user.sign(&refund_msg).to_bytes();
    mgr.claim_htlc_refund(
        &mut state,
        1,
        &to_pubkey(&user),
        400,
        &refund_sig,
    )
    .unwrap();

    assert_eq!(state.metadata.total_claimed, 500_000);
}

// ============================================================================
// AUDIT TEST-5: auto_close_slot and auto_settle
// ============================================================================

/// TEST-5: Auto-close flow — set_auto_close_slot → auto_settle → claim → finalize.
#[test]
fn test_auto_close_slot_and_auto_settle() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Set auto-close at slot 500
    mgr.set_auto_close_slot(&mut state, 500).unwrap();
    assert_eq!(state.metadata.auto_close_slot, Some(500));

    // Before auto_close_slot: auto_settle fails
    let result = mgr.auto_settle(&mut state, 400, 100);
    assert!(result.is_err());

    // At auto_close_slot: auto_settle succeeds
    mgr.auto_settle(&mut state, 500, 100).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);
    assert_eq!(state.metadata.settle_deadline, Some(600));

    // Claim and finalize
    let claim_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 550);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 550, &claim_sig)
        .unwrap();

    let fin_msg = state_message(&state.metadata.channel_id, 600, &state.metadata.current_root);
    let fin_sig = user.sign(&fin_msg).to_bytes();
    let (refund_a, refund_b) = mgr
        .finalize_settlement(&mut state, 600, &to_pubkey(&user), &fin_sig)
        .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Closed);
    assert_eq!(refund_a, 0);
    assert_eq!(refund_b, 0);
}

// ============================================================================
// AUDIT TEST-6: Cross-claim exclusivity tests
// ============================================================================

/// TEST-6a: claim_leaf rejects HTLC leaves (BUG-22), claim_htlc_verify works for HTLC leaves.
#[test]
fn test_claim_leaf_and_htlc_verify_exclusive() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Create HTLC leaf at index 0, owner = user, beneficiary = provider
    let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();
    let htlc_leaf = UTXOLeaf::htlc(
        to_pubkey(&user),
        1_000_000,
        hash_lock,
        5000,
        to_pubkey(&provider),
    );
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        0,
        &prev_leaf,
        htlc_leaf,
        &user,
    );
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();

    // BUG-33: cooperative close rejects HTLC leaves, so use dispute path instead
    // Trigger challenge → settle_after_timeout to reach Settling status
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    mgr.settle_after_timeout(&mut state, 701, 100).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);

    // BUG-22: claim_leaf now rejects HTLC leaves — must use claim_htlc_verify instead
    let claim_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    let result = mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 250, &claim_sig);
    assert!(result.is_err(), "claim_leaf should reject HTLC leaves");

    // Provider claims via claim_htlc_verify — should succeed
    let htlc_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 260);
    let htlc_sig = provider.sign(&htlc_msg).to_bytes();
    mgr.claim_htlc_verify(
        &mut state,
        0,
        &preimage,
        &to_pubkey(&provider),
        260,
        &htlc_sig,
    )
    .unwrap();
    assert_eq!(state.metadata.total_claimed, 1_000_000);
}

/// TEST-6b: claim_leaf rejects HTLC leaves (BUG-22), claim_htlc_refund works for HTLC leaves.
#[test]
fn test_claim_leaf_and_htlc_refund_exclusive() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    // Create HTLC leaf at index 0, timelock = 200
    let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
    let htlc_leaf = UTXOLeaf::htlc(
        to_pubkey(&user),
        1_000_000,
        hash_lock,
        200,
        to_pubkey(&provider),
    );
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        0,
        &prev_leaf,
        htlc_leaf,
        &user,
    );
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();

    // BUG-33: cooperative close rejects HTLC leaves, so use dispute path instead
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    mgr.settle_after_timeout(&mut state, 701, 100).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);

    // BUG-22: claim_leaf now rejects HTLC leaves — must use claim_htlc_refund instead
    let claim_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    let result = mgr.claim_leaf(&mut state, 0, 1_000_000, &to_pubkey(&user), 250, &claim_sig);
    assert!(result.is_err(), "claim_leaf should reject HTLC leaves");

    // User claims via claim_htlc_refund after timelock expires (slot > 200)
    let refund_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 300);
    let refund_sig = user.sign(&refund_msg).to_bytes();
    mgr.claim_htlc_refund(
        &mut state,
        0,
        &to_pubkey(&user),
        300,
        &refund_sig,
    )
    .unwrap();
    assert_eq!(state.metadata.total_claimed, 1_000_000);
}

// ============================================================================
// AUDIT TEST-7: Multi-hop timelock boundary tests
// ============================================================================

/// TEST-7a: Large number of hops (10+) — timelocks should not underflow.
#[test]
fn test_multi_hop_many_hops_no_underflow() {
    use ignite_pay_state_channel::multihop::MultiHopManager;
    use ignite_pay_state_channel::channel::min_timelock;

    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();
    let mgr = MultiHopManager::new(db).unwrap();

    let preimage = [1u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    // 15 hops
    let hops: Vec<_> = (0..15)
        .map(|i| {
            (
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                100_000,
                i,
                [i as u8; 32],
            )
        })
        .collect();

    let challenge_duration = 500u64;
    let payment = mgr
        .create_payment(hash_lock, preimage, hops, 1000, challenge_duration)
        .unwrap();

    // Verify all timelocks are valid (no underflow)
    for (i, hop) in payment.hops.iter().enumerate() {
        assert!(
            hop.timelock_slot > 1000,
            "hop {} timelock {} should be > current_slot 1000",
            i,
            hop.timelock_slot
        );
    }

    // Verify decreasing order
    for i in 1..payment.hops.len() {
        assert!(
            payment.hops[i - 1].timelock_slot > payment.hops[i].timelock_slot,
            "hop {} timelock should be > hop {}",
            i - 1,
            i
        );
    }

    // Last hop's timelock should equal current_slot + min_timelock
    let expected_last = 1000 + min_timelock(challenge_duration);
    assert_eq!(
        payment.hops.last().unwrap().timelock_slot,
        expected_last,
        "last hop timelock should be current_slot + min_timelock(challenge_duration)"
    );
}

/// TEST-7b: claim_leaf_with_proof variant (BUG-14 fix).
#[test]
fn test_claim_leaf_with_external_proof() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Close channel (Settling)
    let signed_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100)
        .unwrap();

    // Generate proof externally (simulating on-chain claim scenario)
    let proof = state.tree.get_proof(0).unwrap();

    let claim_msg = claim_message(&state.metadata.channel_id, 0, 1_000_000, 250);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    mgr.claim_leaf_with_proof(
        &mut state,
        0,
        1_000_000,
        &to_pubkey(&user),
        250,
        &claim_sig,
        &proof,
    )
    .unwrap();
    assert_eq!(state.metadata.total_claimed, 1_000_000);

    // Duplicate claim should fail
    let result = mgr.claim_leaf_with_proof(
        &mut state,
        0,
        1_000_000,
        &to_pubkey(&user),
        250,
        &claim_sig,
        &proof,
    );
    assert!(result.is_err());
}

// ============================================================================
// TEST-9: Compliance + channel integration test
// ============================================================================

/// TEST-9: Verify that compliance module is called during leaf updates
/// when a ComplianceManager is attached to the ChannelManager.
#[test]
fn test_compliance_channel_integration() {
    use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};
    use ignite_pay_state_channel::signing::sign_leaf_update;

    let db = temp_db();
    let user = generate_keypair();
    let provider = generate_keypair();

    // Set up compliance first
    let cm_db = temp_db();
    let cm = ComplianceManager::new(cm_db).unwrap();

    let mut mgr = ChannelManager::new(db).unwrap();
    mgr.set_compliance(cm);

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            10_000_000,
            4,
            100,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            500,
            50, None
        )
        .unwrap();

    // Init compliance for this channel
    mgr.compliance().unwrap().init_channel_compliance(
        state.metadata.channel_id,
        SpendingLimit {
            threshold: 1_000_000,
            per_channel: 10_000_000,
            window_slots: 1000,
        },
    ).unwrap();

    // Create a small leaf update (500K, under threshold)
    let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
    let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 500_000);
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        0,
        &prev_leaf,
        new_leaf,
        &user,
    );

    let result = mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user));
    assert!(result.is_ok(), "Small payment should succeed: {:?}", result);

    // Verify compliance tracked the payment
    let cm_state = mgr.compliance().unwrap()
        .load_state(state.metadata.channel_id).unwrap();
    assert_eq!(cm_state.cumulative_spent, 500_000);
    assert!(!cm_state.compliance_hold);
}

// ============================================================================
// TEST-11: Non-participant leaf claim rejection test
// ============================================================================

/// TEST-11: Verify that a leaf owned by a non-channel participant cannot be
/// claimed by either the user or the provider.
#[test]
fn test_non_participant_leaf_claim_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            10_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split tree: all user-owned leaves initially
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 4_000_000),
    ];
    let _signed = mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // Transfer leaf 0 to a merchant (non-participant) via pipeline
    let merchant = Pubkey::new_unique();
    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        pipeline.transfer_leaf(0, merchant).unwrap();
        let _ = pipeline.build();
    }
    state.tree = tree;
    state.metadata.sequence = seq + 2;
    state.metadata.current_root = state.tree.root();

    // Close channel to enter Settling
    let new_root = state.metadata.current_root;
    let new_seq = state.metadata.sequence;
    let signed_close = SignedState {
        channel_id,
        sequence: new_seq,
        root: new_root,
        sig_a: sign_state(&channel_id, new_seq, &new_root, &user),
        sig_b: sign_state(&channel_id, new_seq, &new_root, &provider),
    };
    mgr.close_channel(&mut state, &signed_close, &to_pubkey(&user), &to_pubkey(&provider), 200, 100)
        .unwrap();

    // User tries to claim the merchant's leaf (leaf 0) — should be rejected
    // because the user doesn't own it anymore
    let claim_msg = claim_message(&channel_id, 0, 3_000_000, 250);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    let result = mgr.claim_leaf(
        &mut state,
        0,
        3_000_000,
        &to_pubkey(&user),
        250,
        &claim_sig,
    );
    assert!(result.is_err(), "User should not be able to claim merchant's leaf");

    // Provider also tries — should also be rejected
    let claim_sig_prov = provider.sign(&claim_msg).to_bytes();
    let result2 = mgr.claim_leaf(
        &mut state,
        0,
        3_000_000,
        &to_pubkey(&provider),
        250,
        &claim_sig_prov,
    );
    assert!(result2.is_err(), "Provider should not be able to claim merchant's leaf");

    // User can claim their own leaf (leaf 1, still owned by user)
    let claim_msg2 = claim_message(&channel_id, 1, 3_000_000, 250);
    let claim_sig2 = user.sign(&claim_msg2).to_bytes();
    mgr.claim_leaf(
        &mut state,
        1,
        3_000_000,
        &to_pubkey(&user),
        250,
        &claim_sig2,
    ).unwrap();
    assert_eq!(state.metadata.total_claimed, 3_000_000);
}

// ============================================================================
// TEST-12: compute_hop_amounts overflow test
// ============================================================================

/// TEST-12: Verify compute_hop_amounts returns None on overflow.
#[test]
fn test_compute_hop_amounts_overflow() {
    use ignite_pay_state_channel::multihop::compute_hop_amounts;

    // With destination_amount near u64::MAX and multiple hops with fees,
    // the backward computation should overflow
    let near_max = u64::MAX - 1000;
    let result = compute_hop_amounts(near_max, &[100, 100, 100]);
    assert!(result.is_none(), "Should overflow with near-max amount and fees");

    // Single hop with max amount and zero fee should succeed
    let result2 = compute_hop_amounts(u64::MAX, &[0]);
    assert!(result2.is_some(), "Single hop with 0 bps fee should not overflow");
    assert_eq!(result2.unwrap()[0], u64::MAX);

    // Large amount with small fee should work
    let result3 = compute_hop_amounts(1_000_000_000, &[1, 1]);
    assert!(result3.is_some());
}

// ============================================================================
// TEST-13: merge_spent_leaves overflow boundary test
// ============================================================================

/// TEST-13: Verify that merging leaves with large amounts uses saturating arithmetic.
#[test]
fn test_merge_spent_leaves_overflow_boundary() {
    use ignite_pay_state_channel::helpers::merge_spent_leaves;

    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();
    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    // Use a 3-depth tree (8 leaves). Put large amounts in 2 leaves.
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            u64::MAX,
            3,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split tree: one leaf has u64::MAX, rest is empty
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), u64::MAX),
        UTXOLeaf::empty(),
        UTXOLeaf::empty(),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // Merge leaf 0 (u64::MAX) into leaf 1 — saturating_add should cap at u64::MAX
    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    let result = merge_spent_leaves(
        &channel_id,
        &[0],
        1,
        seq + 1,
        &mut tree,
        &user,
    );
    assert!(result.is_ok(), "Merge with u64::MAX should succeed via saturating_add");

    // Target leaf amount should be u64::MAX (0 + u64::MAX = u64::MAX, saturating)
    let target = tree.get_leaf(1).unwrap();
    assert_eq!(target.amount, u64::MAX);
}

// ============================================================================
// TEST-14: merge_spent_leaves target==source conflict test
// ============================================================================

/// TEST-14: Verify that merging fails when target index is in source indices.
#[test]
fn test_merge_target_source_conflict() {
    use ignite_pay_state_channel::helpers::merge_spent_leaves;

    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();
    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 500_000),
        UTXOLeaf::standard(to_pubkey(&user), 500_000),
        UTXOLeaf::empty(),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    // Try merging [0, 1] into target 0 — target is also a source
    let result = merge_spent_leaves(
        &channel_id,
        &[0, 1],
        0, // target == source
        seq + 1,
        &mut tree,
        &user,
    );
    assert!(result.is_err(), "Merge should fail when target index is in source indices");
}

// ============================================================================
// TEST-15: routing fee overflow test
// ============================================================================

/// TEST-15: Verify that route scoring handles large amounts + high fee rates without overflow.
#[test]
fn test_routing_fee_overflow() {
    use ignite_pay_state_channel::routing::RouteService;
    use ignite_pay_state_channel::hub::{HubManager, HubMetrics, HubLeaf};

    let db = temp_db();
    let hub_mgr = HubManager::new(db).unwrap();

    let hub1_did = [1u8; 32];
    let hub2_did = [2u8; 32];

    hub_mgr.register_hub(HubLeaf {
        hub_did_hash: hub1_did,
        active_pubkey: Pubkey::new_unique(),
        endpoint_hash: [0u8; 32],
        collateral: 10_000_000,
        platform_vc_hash: [0u8; 32],
        metrics_hash: [0u8; 32],
        slot_updated: 100,
    }).unwrap();

    hub_mgr.register_hub(HubLeaf {
        hub_did_hash: hub2_did,
        active_pubkey: Pubkey::new_unique(),
        endpoint_hash: [0u8; 32],
        collateral: 5_000_000,
        platform_vc_hash: [0u8; 32],
        metrics_hash: [0u8; 32],
        slot_updated: 100,
    }).unwrap();

    // High fee rate (65535 bps = 655.35%) with large amounts
    hub_mgr.update_metrics(hub1_did, HubMetrics {
        online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
        total_routed: 1_000_000, total_transactions: 100, active_channels: 10,
        available_liquidity: 50_000_000, fee_rate_bps: 65535,
    }).unwrap();

    hub_mgr.update_metrics(hub2_did, HubMetrics {
        online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
        total_routed: 1_000_000, total_transactions: 100, active_channels: 10,
        available_liquidity: 50_000_000, fee_rate_bps: 65535,
    }).unwrap();

    // score_route with large amount should not panic or overflow
    let metrics1 = hub_mgr.get_metrics(hub1_did).unwrap().unwrap();
    let metrics2 = hub_mgr.get_metrics(hub2_did).unwrap().unwrap();

    // This would overflow with plain multiplication: u64::MAX * 65535
    let score = RouteService::score_route(&[&metrics1, &metrics2], u64::MAX);
    assert!(score.is_finite(), "Score should be finite even with u64::MAX amount + high fee_rate");
    assert!(score >= 0.0, "Score should be non-negative");
}

// ============================================================================
// TEST-17: trigger_challenge submitted_sequence == current_sequence boundary test
// ============================================================================

/// TEST-17: Verify trigger_challenge rejects submitted_sequence == current_sequence.
#[test]
fn test_trigger_challenge_sequence_equal_boundary() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Try to trigger challenge with submitted_sequence == current_sequence (0)
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence; // 0 == current
    let msg = ignite_pay_state_channel::signing::state_message(
        &state.metadata.channel_id, submitted_sequence, &submitted_root,
    );
    let sig = user.sign(&msg).to_bytes();
    let result = mgr.trigger_challenge(
        &mut state,
        &to_pubkey(&user),
        200,
        &submitted_root,
        submitted_sequence,
        &sig,
    );
    assert!(result.is_err(), "trigger_challenge should reject submitted_sequence == current_sequence");
}

// ============================================================================
// TEST-18: compliance slot=0 window behavior test
// ============================================================================

/// TEST-18: Verify that compliance record_payment with slot=0 uses cumulative_spent
/// for threshold checking (not window_spend which would be empty).
#[test]
fn test_compliance_slot_zero_uses_cumulative() {
    use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

    let db = temp_db();
    let cm = ComplianceManager::new(db).unwrap();
    let channel_id = [42u8; 32];

    // Threshold 500K, per_channel 10M, window 1000 slots
    cm.init_channel_compliance(channel_id, SpendingLimit {
        threshold: 500_000,
        per_channel: 10_000_000,
        window_slots: 1000,
    }).unwrap();

    let user = Pubkey::new_unique();
    let provider = Pubkey::new_unique();

    // Record payments with slot=0 (as channel.rs does)
    let action1 = cm.record_payment(channel_id, 300_000, 0, user, provider).unwrap();
    assert!(matches!(action1, ignite_pay_state_channel::compliance::ComplianceAction::None));

    // Cumulative is now 300K, not yet at threshold
    let state = cm.load_state(channel_id).unwrap();
    assert_eq!(state.cumulative_spent, 300_000);
    assert!(!state.compliance_hold);
    assert!(state.window_payments.is_empty(), "slot=0 payments should not be added to window");

    // Record another 300K with slot=0 — cumulative 600K > threshold 500K
    let action2 = cm.record_payment(channel_id, 300_000, 0, user, provider).unwrap();
    match action2 {
        ignite_pay_state_channel::compliance::ComplianceAction::InsertMarker { threshold, .. } => {
            assert_eq!(threshold, 500_000);
        }
        ignite_pay_state_channel::compliance::ComplianceAction::None => {
            panic!("Should trigger InsertMarker when cumulative exceeds threshold with slot=0");
        }
    }

    let state2 = cm.load_state(channel_id).unwrap();
    assert!(state2.compliance_hold);
    assert_eq!(state2.cumulative_spent, 600_000);
    assert!(state2.window_payments.is_empty(), "slot=0 payments should never be in window");
}

// ============================================================================
// AUDIT FIX-1: SubmitCounterState with counter_leaves tree rebuild
// ============================================================================

/// Verify that submit_counter_state correctly rebuilds the MerkleTree when
/// counter_leaves are provided, and rejects if the reconstructed root doesn't
/// match the signed root.
#[test]
fn test_submit_counter_state_with_leaves_rebuild() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split tree first
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 400_000),
        UTXOLeaf::standard(to_pubkey(&user), 600_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // Trigger challenge
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();

    // Build a valid counter-state with real leaves
    let counter_leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 300_000),
        UTXOLeaf::standard(to_pubkey(&user), 700_000),
    ];
    let counter_tree = MerkleTree::new(counter_leaves.clone(), state.metadata.tree_depth as usize).unwrap();
    let counter_root = counter_tree.root();

    let counter_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: 10,
        root: counter_root,
        sig_a: sign_state(&state.metadata.channel_id, 10, &counter_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, 10, &counter_root, &provider),
    };

    mgr.submit_counter_state(&mut state, &counter_state, Some(counter_leaves), &to_pubkey(&user), &to_pubkey(&provider))
        .unwrap();

    assert_eq!(state.metadata.sequence, 10);
    assert_eq!(state.metadata.current_root, counter_root);
    assert_eq!(state.tree.get_leaf(0).unwrap().amount, 300_000);
    assert_eq!(state.tree.get_leaf(1).unwrap().amount, 700_000);
    assert_eq!(state.tree.total_amount(), 1_000_000);
}

/// Verify that submit_counter_state rejects counter_leaves whose root doesn't
/// match the signed root.
#[test]
fn test_submit_counter_state_wrong_leaves_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Trigger challenge
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();

    // Build counter-state with a fake root
    let fake_root = [99u8; 32];
    let counter_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: 5,
        root: fake_root,
        sig_a: sign_state(&state.metadata.channel_id, 5, &fake_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, 5, &fake_root, &provider),
    };

    // Provide leaves that DON'T match the fake root
    let wrong_leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 500_000),
        UTXOLeaf::standard(to_pubkey(&user), 500_000),
    ];

    let result = mgr.submit_counter_state(&mut state, &counter_state, Some(wrong_leaves), &to_pubkey(&user), &to_pubkey(&provider));
    assert!(result.is_err(), "Should reject leaves whose root doesn't match signed root");
}

// ============================================================================
// AUDIT FIX-2: Cooperative close then re-close should be rejected
// ============================================================================

#[test]
fn test_close_channel_then_reclose_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // First close
    let signed_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);

    // Second close attempt should fail
    let signed_state2 = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    let result = mgr.close_channel(&mut state, &signed_state2, &to_pubkey(&user), &to_pubkey(&provider), 300, 100);
    assert!(result.is_err(), "Closing an already-settling channel should be rejected");
}

// ============================================================================
// AUDIT FIX-3: Dual-funded + cooperative close + proportional refund E2E
// ============================================================================

#[test]
fn test_dual_funded_close_proportional_refund() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    // Open with user deposit 1_000_000
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Provider funds 500_000
    mgr.fund_channel(&mut state, &provider, 500_000, None).unwrap();
    assert_eq!(state.metadata.deposit_a, 1_000_000);
    assert_eq!(state.metadata.deposit_b, 500_000);
    assert_eq!(state.metadata.total_deposited, 1_500_000);

    // Split tree with both parties
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 600_000),
        UTXOLeaf::standard(to_pubkey(&user), 400_000),
        UTXOLeaf::standard(to_pubkey(&provider), 500_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // Cooperative close
    let signed_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

    // Claim only user's leaf 0 (600K)
    let claim_msg = claim_message(&state.metadata.channel_id, 0, 600_000, 250);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    mgr.claim_leaf(&mut state, 0, 600_000, &to_pubkey(&user), 250, &claim_sig).unwrap();
    assert_eq!(state.metadata.total_claimed, 600_000);

    // Finalize — unclaimed = 1_500_000 - 600_000 = 900_000
    // Proportional: deposit_a = 1_000_000, deposit_b = 500_000, total = 1_500_000
    // refund_a = 900_000 * 1_000_000 / 1_500_000 = 600_000
    // refund_b = 900_000 - 600_000 = 300_000
    let fin_msg = state_message(&state.metadata.channel_id, 300, &state.metadata.current_root);
    let fin_sig = user.sign(&fin_msg).to_bytes();
    let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 300, &to_pubkey(&user), &fin_sig).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Closed);
    assert_eq!(refund_a, 600_000);
    assert_eq!(refund_b, 300_000);
}

// ============================================================================
// AUDIT FIX-4: Multiple leaf claims then FinalizeSettlement
// ============================================================================

#[test]
fn test_multiple_leaf_claims_then_finalize() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            10_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split into 4 leaves
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 1_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 4_000_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    // Close
    let signed_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100).unwrap();

    // Claim leaves 0, 1, and 3 (2M + 3M + 4M = 9M)
    for (idx, amount) in [(0, 2_000_000), (1, 3_000_000), (3, 4_000_000)] {
        let claim_msg = claim_message(&state.metadata.channel_id, idx as u32, amount, 250);
        let claim_sig = user.sign(&claim_msg).to_bytes();
        mgr.claim_leaf(&mut state, idx, amount, &to_pubkey(&user), 250, &claim_sig).unwrap();
    }
    assert_eq!(state.metadata.total_claimed, 9_000_000);

    // Finalize — unclaimed = 10M - 9M = 1M
    let fin_msg = state_message(&state.metadata.channel_id, 300, &state.metadata.current_root);
    let fin_sig = user.sign(&fin_msg).to_bytes();
    let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 300, &to_pubkey(&user), &fin_sig).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Closed);
    assert_eq!(refund_a, 1_000_000);
    assert_eq!(refund_b, 0);
}

// ============================================================================
// AUDIT FIX-5: Challenged → TriggerChallenge again should be rejected
// ============================================================================

#[test]
fn test_trigger_challenge_when_already_challenged_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // First challenge
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Challenged);

    // Second challenge attempt should fail
    let submitted_root2 = state.metadata.current_root;
    let submitted_sequence2 = state.metadata.sequence + 1;
    let msg2 = state_message(&state.metadata.channel_id, submitted_sequence2, &submitted_root2);
    let sig2 = provider.sign(&msg2).to_bytes();
    let result = mgr.trigger_challenge(&mut state, &to_pubkey(&provider), 300, &submitted_root2, submitted_sequence2, &sig2);
    assert!(result.is_err(), "trigger_challenge on already-challenged channel should be rejected");
}

// ============================================================================
// AUDIT FIX-6: Multi-hop resolve_hop integration test
// ============================================================================

#[test]
fn test_multihop_resolve_hop_integration() {
    use ignite_pay_state_channel::multihop::MultiHopManager;

    let dir = tempfile::tempdir().unwrap();
    let db = sled::open(dir.path()).unwrap();
    let mgr = MultiHopManager::new(db).unwrap();

    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();

    // 3 hops
    let hops: Vec<_> = (0..3)
        .map(|i| {
            (
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                100_000 + i as u64 * 10_000,
                i,
                [i as u8; 32],
            )
        })
        .collect();

    let payment = mgr.create_payment(hash_lock, preimage, hops, 1000, 500).unwrap();
    let payment_id = payment.payment_id;

    // Cannot resolve before revealing preimage
    let result = mgr.resolve_hop(&payment_id, 0);
    assert!(result.is_err(), "resolve_hop should fail before reveal_preimage");

    // Reveal preimage
    let payment = mgr.reveal_preimage(&payment_id, &preimage).unwrap();
    assert_eq!(payment.status, ignite_pay_state_channel::multihop::MultiHopStatus::Resolving);

    // Resolve hops one by one
    let p1 = mgr.resolve_hop(&payment_id, 0).unwrap();
    assert!(p1.hops[0].resolved);
    assert!(!p1.hops[1].resolved);
    assert_eq!(p1.status, ignite_pay_state_channel::multihop::MultiHopStatus::Resolving);

    let p2 = mgr.resolve_hop(&payment_id, 1).unwrap();
    assert!(p2.hops[1].resolved);
    assert_eq!(p2.status, ignite_pay_state_channel::multihop::MultiHopStatus::Resolving);

    // Resolve last hop — should complete the payment
    let p3 = mgr.resolve_hop(&payment_id, 2).unwrap();
    assert!(p3.hops[2].resolved);
    assert_eq!(p3.status, ignite_pay_state_channel::multihop::MultiHopStatus::Completed);

    // Verify persistence
    let loaded = mgr.load_payment(&payment_id).unwrap();
    assert_eq!(loaded.status, ignite_pay_state_channel::multihop::MultiHopStatus::Completed);
    assert!(loaded.hops.iter().all(|h| h.resolved));
}

// ============================================================================
// AUDIT FIX-7: Routing graph with cycles and isolated nodes
// ============================================================================

#[test]
fn test_routing_cycles_and_isolated_nodes() {
    use ignite_pay_state_channel::routing::RouteService;
    use ignite_pay_state_channel::hub::{HubManager, HubLeaf, HubMetrics};

    let db = temp_db();
    let hub_mgr = HubManager::new(db).unwrap();

    // 4 hubs: 1-2-3 form a chain, 4 is isolated
    let hub1 = [1u8; 32];
    let hub2 = [2u8; 32];
    let hub3 = [3u8; 32];
    let hub4 = [4u8; 32]; // isolated

    for (did, collateral) in [(hub1, 10_000_000), (hub2, 10_000_000), (hub3, 10_000_000), (hub4, 10_000_000)] {
        hub_mgr.register_hub(HubLeaf {
            hub_did_hash: did,
            active_pubkey: Pubkey::new_unique(),
            endpoint_hash: [0u8; 32],
            collateral,
            platform_vc_hash: [0u8; 32],
            metrics_hash: [0u8; 32],
            slot_updated: 100,
        }).unwrap();
    }

    // Set metrics for all hubs
    let default_metrics = HubMetrics {
        online_rate: 9900, success_rate: 9950, avg_latency_ms: 30,
        total_routed: 1_000_000, total_transactions: 100, active_channels: 10,
        available_liquidity: 50_000_000, fee_rate_bps: 100,
    };
    for did in [hub1, hub2, hub3, hub4] {
        hub_mgr.update_metrics(did, default_metrics.clone()).unwrap();
    }

    let mut route_svc = RouteService::new(hub_mgr);

    // Chain: 1 → 2 → 3 (and back for cycles: 3 → 2 → 1)
    route_svc.add_channel_edge(hub1, hub2);
    route_svc.add_channel_edge(hub2, hub3);
    // Add a cycle: 3 → 1
    route_svc.add_channel_edge(hub3, hub1);

    // Find route from 1 to 3 — should find 1→2→3 (not cycle through 1 again)
    let routes = route_svc.discover_routes(&ignite_pay_state_channel::routing::RouteRequest {
        from_did_hash: hub1,
        to_did_hash: hub3,
        amount: 100_000,
        token_mint: Pubkey::new_unique(),
        max_hops: 5,
    }).unwrap();

    assert!(!routes.is_empty(), "Should find at least one route from hub1 to hub3");
    // All routes should end at hub3
    for route in &routes {
        assert!(route.hops.last().map(|h| h.hub_did_hash) == Some(hub3));
    }

    // No route to isolated hub4
    let routes_to_isolated = route_svc.discover_routes(&ignite_pay_state_channel::routing::RouteRequest {
        from_did_hash: hub1,
        to_did_hash: hub4,
        amount: 100_000,
        token_mint: Pubkey::new_unique(),
        max_hops: 5,
    }).unwrap();
    assert!(routes_to_isolated.is_empty(), "Should not find route to isolated hub");
}

// ============================================================================
// AUDIT FIX-8: Compliance hold clear then resume payment test
// ============================================================================

#[test]
fn test_compliance_hold_clear_then_resume() {
    use ignite_pay_state_channel::compliance::{ComplianceManager, SpendingLimit};

    let db = temp_db();
    let cm = ComplianceManager::new(db).unwrap();
    let channel_id = [88u8; 32];

    // Threshold 500K
    cm.init_channel_compliance(channel_id, SpendingLimit {
        threshold: 500_000,
        per_channel: 10_000_000,
        window_slots: 1000,
    }).unwrap();

    let user = Pubkey::new_unique();
    let provider = Pubkey::new_unique();

    // Trigger hold by exceeding threshold
    let action = cm.record_payment(channel_id, 600_000, 100, user, provider).unwrap();
    assert!(matches!(action, ignite_pay_state_channel::compliance::ComplianceAction::InsertMarker { .. }));
    assert!(cm.load_state(channel_id).unwrap().compliance_hold);

    // Payment should be rejected while on hold
    let result = cm.record_payment(channel_id, 100_000, 200, user, provider);
    assert!(result.is_err(), "Payment should be rejected while compliance hold is active");

    // Clear the hold
    cm.clear_hold(channel_id).unwrap();
    assert!(!cm.load_state(channel_id).unwrap().compliance_hold);

    // Payment should succeed after hold is cleared.
    // Use a slot far enough in the future that the window has expired,
    // so the new small payment won't re-trigger the threshold.
    let action2 = cm.record_payment(channel_id, 100_000, 2000, user, provider).unwrap();
    // The window is empty (all old payments pruned), so 100K < 500K threshold → None
    assert!(matches!(action2, ignite_pay_state_channel::compliance::ComplianceAction::None));

    let state = cm.load_state(channel_id).unwrap();
    assert_eq!(state.cumulative_spent, 700_000);
    assert!(!state.compliance_hold);
}

// ============================================================================
// AUDIT FIX-9: Batch duplicate leaf_index rejected with clear error
// ============================================================================

#[test]
fn test_batch_duplicate_leaf_index_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let signer = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&signer),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            3,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split to get multiple leaves
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&signer), 400_000),
        UTXOLeaf::standard(to_pubkey(&signer), 600_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &signer, &provider).unwrap();

    // Create two updates both targeting leaf 0
    let prev0 = state.tree.get_leaf(0).unwrap().clone();
    let new0a = UTXOLeaf::standard(Pubkey::new_unique(), 200_000);
    let update0a = sign_leaf_update(&state.metadata.channel_id, 2, 0, &prev0, new0a, &signer);

    let new0b = UTXOLeaf::standard(Pubkey::new_unique(), 100_000);
    let update0b = sign_leaf_update(&state.metadata.channel_id, 3, 0, &prev0, new0b, &signer);

    let result = mgr.apply_leaf_update_batch(
        &mut state,
        &[update0a, update0b],
        &to_pubkey(&signer),
    );
    assert!(result.is_err(), "Batch with duplicate leaf_index should be rejected");
}

// ============================================================================
// AUDIT FIX-10: fund_channel deposit_b overflow cap
// ============================================================================

#[test]
fn test_fund_channel_deposit_overflow_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Try funding with u64::MAX — should overflow total_deposited
    let result = mgr.fund_channel(&mut state, &provider, u64::MAX, None);
    assert!(result.is_err(), "fund_channel should reject deposit_b that would overflow total_deposited");
}

// ============================================================================
// T-1: Close channel with unresolved HTLC should be rejected (BUG-33)
// ============================================================================

#[test]
fn test_close_channel_with_htlc_rejected() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            1_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Create HTLC leaf at index 0
    let prev_leaf = state.tree.get_leaf(0).unwrap().clone();
    let preimage = [42u8; 32];
    let hash_lock = solana_hash(&preimage).to_bytes();
    let htlc_leaf = UTXOLeaf::htlc(
        to_pubkey(&user),
        1_000_000,
        hash_lock,
        5000,
        to_pubkey(&provider),
    );
    let update = sign_leaf_update(
        &state.metadata.channel_id,
        1,
        0,
        &prev_leaf,
        htlc_leaf,
        &user,
    );
    mgr.apply_leaf_update(&mut state, &update, &to_pubkey(&user))
        .unwrap();

    // Attempt cooperative close with HTLC still active — should fail
    let signed_state = SignedState {
        channel_id: state.metadata.channel_id,
        sequence: state.metadata.sequence,
        root: state.metadata.current_root,
        sig_a: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &user),
        sig_b: sign_state(&state.metadata.channel_id, state.metadata.sequence, &state.metadata.current_root, &provider),
    };
    let result = mgr.close_channel(&mut state, &signed_state, &to_pubkey(&user), &to_pubkey(&provider), 200, 100);
    assert!(result.is_err(), "close_channel should reject when HTLC leaves exist");
}

// ============================================================================
// T-2: Multiple concurrent HTLC lifecycle
// ============================================================================

#[test]
fn test_multiple_concurrent_htlcs() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            10_000_000,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Split tree into 4 leaves
    let leaves = vec![
        UTXOLeaf::standard(to_pubkey(&user), 2_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 3_000_000),
        UTXOLeaf::standard(to_pubkey(&user), 5_000_000),
    ];
    mgr.construct_split_tree(&mut state, leaves, &user, &provider).unwrap();

    let channel_id = state.metadata.channel_id;
    let seq = state.metadata.sequence;
    let mut tree = state.tree;

    // Create 3 HTLCs on leaves 0, 1, 2 with different preimages
    let preimage1 = [1u8; 32];
    let preimage2 = [2u8; 32];
    let preimage3 = [3u8; 32];
    let hash_lock1 = solana_hash(&preimage1).to_bytes();
    let hash_lock2 = solana_hash(&preimage2).to_bytes();
    let hash_lock3 = solana_hash(&preimage3).to_bytes();

    let beneficiary1 = Pubkey::new_unique();
    let beneficiary2 = Pubkey::new_unique();
    let beneficiary3 = Pubkey::new_unique();

    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 1, &user);
        pipeline.create_htlc(0, hash_lock1, 2000, beneficiary1, 100, 500).unwrap();
        pipeline.create_htlc(1, hash_lock2, 3000, beneficiary2, 100, 500).unwrap();
        pipeline.create_htlc(2, hash_lock3, 4000, beneficiary3, 100, 500).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 3);
    }

    // Verify all 3 leaves are HTLC
    assert_eq!(tree.get_leaf(0).unwrap().leaf_type, LeafType::HTLC);
    assert_eq!(tree.get_leaf(1).unwrap().leaf_type, LeafType::HTLC);
    assert_eq!(tree.get_leaf(2).unwrap().leaf_type, LeafType::HTLC);
    assert_eq!(tree.total_amount(), 10_000_000);

    // Resolve HTLC 0 and 2, refund HTLC 1
    {
        let mut pipeline = Pipeline::new(&mut tree, channel_id, seq + 4, &user);
        pipeline.resolve_htlc(0, &preimage1).unwrap();
        pipeline.refund_htlc(1, 3100).unwrap(); // slot > 3000
        pipeline.resolve_htlc(2, &preimage3).unwrap();

        let (updates, _) = pipeline.build();
        assert_eq!(updates.len(), 3);
    }

    // Verify final state
    assert_eq!(tree.get_leaf(0).unwrap().leaf_type, LeafType::Standard);
    assert_eq!(tree.get_leaf(0).unwrap().owner, beneficiary1);
    assert_eq!(tree.get_leaf(0).unwrap().amount, 2_000_000);

    assert_eq!(tree.get_leaf(1).unwrap().leaf_type, LeafType::Standard);
    assert_eq!(tree.get_leaf(1).unwrap().owner, to_pubkey(&user)); // refunded
    assert_eq!(tree.get_leaf(1).unwrap().amount, 3_000_000);

    assert_eq!(tree.get_leaf(2).unwrap().leaf_type, LeafType::Standard);
    assert_eq!(tree.get_leaf(2).unwrap().owner, beneficiary3);
    assert_eq!(tree.get_leaf(2).unwrap().amount, 5_000_000);

    assert_eq!(tree.total_amount(), 10_000_000);
}

// ============================================================================
// T-11: Negotiation failure — full refund based on Root_init
// ============================================================================

#[test]
fn test_negotiation_failure_full_refund() {
    let db = temp_db();
    let mgr = ChannelManager::new(db).unwrap();

    let user = generate_keypair();
    let provider = generate_keypair();
    let (vault_a, vault_b) = default_vaults();

    let deposit = 1_000_000;
    let mut state = mgr
        .open_channel(
            &to_pubkey(&user),
            &to_pubkey(&provider),
            &Pubkey::new_unique(),
            deposit,
            4,
            100,
            &vault_a,
            &vault_b,
            500,
            50, None
        )
        .unwrap();

    // Simulate negotiation failure: user proposes split but provider refuses
    // User triggers challenge to dispute the channel
    let submitted_root = state.metadata.current_root;
    let submitted_sequence = state.metadata.sequence + 1;
    let msg = state_message(&state.metadata.channel_id, submitted_sequence, &submitted_root);
    let sig = user.sign(&msg).to_bytes();
    mgr.trigger_challenge(&mut state, &to_pubkey(&user), 200, &submitted_root, submitted_sequence, &sig)
        .unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Challenged);

    // Provider does not submit counter-state, so settle after timeout
    mgr.settle_after_timeout(&mut state, 701, 100).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Settling);

    // User claims their full deposit back (the entire tree is still user-owned)
    let claim_msg = claim_message(&state.metadata.channel_id, 0, deposit, 750);
    let claim_sig = user.sign(&claim_msg).to_bytes();
    mgr.claim_leaf(&mut state, 0, deposit, &to_pubkey(&user), 750, &claim_sig).unwrap();

    // Finalize
    let fin_msg = state_message(&state.metadata.channel_id, 801, &state.metadata.current_root);
    let fin_sig = user.sign(&fin_msg).to_bytes();
    let (refund_a, refund_b) = mgr.finalize_settlement(&mut state, 801, &to_pubkey(&user), &fin_sig).unwrap();
    assert_eq!(state.metadata.status, ChannelStatus::Closed);
    assert_eq!(refund_a, 0); // all claimed
    assert_eq!(refund_b, 0);
}

// ============================================================================
// T-13: Multi-hop fee precision tests
// ============================================================================

#[test]
fn test_multihop_fee_precision() {
    use ignite_pay_state_channel::multihop::compute_hop_amounts;

    // Small amount with small fee
    let result = compute_hop_amounts(100, &[10]); // 0.1% fee
    assert!(result.is_some());
    let amounts = result.unwrap();
    assert_eq!(amounts.len(), 1);
    assert!(amounts[0] >= 100);

    // Zero fee
    let result2 = compute_hop_amounts(1_000_000, &[0, 0, 0]);
    assert!(result2.is_some());
    let amounts2 = result2.unwrap();
    assert_eq!(amounts2.len(), 3);
    assert_eq!(amounts2[0], 1_000_000); // no fee = source pays destination amount
    assert_eq!(amounts2[1], amounts2[0]); // same amount through zero-fee hops
    assert_eq!(amounts2[2], amounts2[1]);

    // Large amount with moderate fee
    let result3 = compute_hop_amounts(1_000_000_000_000, &[50, 50]); // 0.5% each
    assert!(result3.is_some());
    let amounts3 = result3.unwrap();
    assert_eq!(amounts3.len(), 2);
    // First hop >= second hop (fees accumulate backward)
    assert!(amounts3[0] >= amounts3[1]);
}
