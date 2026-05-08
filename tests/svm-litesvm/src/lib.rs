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

#![allow(dead_code, unused_imports)]

use solana_account::{Account, ReadableAccount, WritableAccount};
use solana_address::Address;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_sha256_hasher::{hash, hashv};
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::str::FromStr;

type Pubkey = Address;
type TxResult = litesvm::types::TransactionResult;

const PROGRAM_ID_STR: &str = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe";
const TOKEN_PROG: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const RENT_SYSVAR: &str = "SysvarRent111111111111111111111111111111111";
const CLOCK_SYSVAR: &str = "SysvarC1ock11111111111111111111111111111111";
const SYS_PROGRAM: &str = "11111111111111111111111111111111";
const INSTRUCTIONS_SYSVAR: &str = "Sysvar1nstructions1111111111111111111111111";

fn kp(seed: u8) -> Keypair {
    let mut s = [0u8; 32];
    s[0] = seed;
    Keypair::new_from_array(s)
}

fn ixDisc(name: &str) -> [u8; 8] {
    let h = hash(format!("global:{}", name).as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

fn sign_ed25519(message: &[u8], keypair: &Keypair) -> [u8; 64] {
    *keypair.sign_message(message).as_array()
}

fn build_ed25519_ix(public_key: &Pubkey, message: &[u8], signature: &[u8; 64]) -> Instruction {
    let ed25519_pid = Pubkey::from_str("Ed25519SigVerify1111111111111111111111111111").unwrap();
    let data_start: u16 = 16;
    let sig_offset = data_start;
    let pk_offset = sig_offset + 64;
    let msg_offset = pk_offset + 32;
    let msg_size = message.len() as u16;
    let self_ix_index = u16::MAX;
    let mut data = Vec::with_capacity(16 + 64 + 32 + message.len());
    data.push(1u8);
    data.push(0u8);
    data.extend_from_slice(&sig_offset.to_le_bytes());
    data.extend_from_slice(&self_ix_index.to_le_bytes());
    data.extend_from_slice(&pk_offset.to_le_bytes());
    data.extend_from_slice(&self_ix_index.to_le_bytes());
    data.extend_from_slice(&msg_offset.to_le_bytes());
    data.extend_from_slice(&msg_size.to_le_bytes());
    data.extend_from_slice(&self_ix_index.to_le_bytes());
    data.extend_from_slice(signature);
    data.extend_from_slice(public_key.as_ref());
    data.extend_from_slice(message);
    Instruction { program_id: ed25519_pid, accounts: vec![], data }
}

fn build_simple_leaf(owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut b = vec![0u8; 41];
    b[0] = 0;
    b[1..33].copy_from_slice(owner.as_ref());
    b[33..41].copy_from_slice(&amount.to_le_bytes());
    b
}

fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() { return [0u8; 32]; }
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::new();
        let mut i = 0;
        while i < layer.len() {
            let left = layer[i];
            let right = if i + 1 < layer.len() { layer[i + 1] } else { [0u8; 32] };
            let (a, b) = if left < right { (left, right) } else { (right, left) };
            next.push(hashv(&[&a, &b]).to_bytes());
            i += 2;
        }
        layer = next;
    }
    layer[0]
}

fn make_mint_acct(authority: &Pubkey, decimals: u8) -> Account {
    let sys = Pubkey::from_str(SYS_PROGRAM).unwrap();
    let mut acct = Account::new(2_000_000, 82, &sys);
    let d = acct.data_as_mut_slice();
    d[0..4].copy_from_slice(&1u32.to_le_bytes());
    d[4..36].copy_from_slice(authority.as_ref());
    d[44] = decimals;
    d[45] = 1;
    acct
}

fn make_token_acct(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Account {
    let tp = Pubkey::from_str(TOKEN_PROG).unwrap();
    let mut acct = Account::new(2_000_000, 165, &tp);
    let d = acct.data_as_mut_slice();
    d[0..32].copy_from_slice(mint.as_ref());
    d[32..64].copy_from_slice(owner.as_ref());
    d[64..72].copy_from_slice(&amount.to_le_bytes());
    d[108] = 1;
    acct
}

fn setup_svm() -> (litesvm::LiteSVM, Pubkey) {
    let mut svm = litesvm::LiteSVM::new();
    let pid = Pubkey::from_str(PROGRAM_ID_STR).unwrap();
    let sbf_out = std::env::var("SBF_OUT_DIR")
        .unwrap_or_else(|_| "target/deploy".to_string());
    let sbf_out = if std::path::Path::new(&sbf_out).is_absolute() {
        sbf_out
    } else {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set");
        let project_root = std::path::Path::new(&manifest_dir)
            .parent().unwrap()
            .parent().unwrap();
        project_root.join(&sbf_out).to_str().unwrap().to_string()
    };
    let so_path = format!("{}/ignite_pay_program.so", sbf_out);
    let bytes = std::fs::read(&so_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {} — run 'make build-sbf' or set SBF_OUT_DIR", so_path, e));
    svm.add_program(pid, &bytes).expect("add_program failed");

    // Register a noop program as the ed25519 precompile so that
    // ed25519 verification instructions can be included in transactions.
    // LiteSVM doesn't support ed25519 precompiles natively, so we use
    // a trivial SBPF program that always succeeds.
    let ed25519_pid = Pubkey::from_str("Ed25519SigVerify1111111111111111111111111111").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let noop_path = std::path::Path::new(&manifest_dir).join("noop_ed25519.so");
    let noop_bytes = std::fs::read(&noop_path)
        .expect("noop_ed25519.so not found in test directory");
    svm.add_program(ed25519_pid, &noop_bytes).expect("add_program ed25519 noop failed");

    (svm, pid)
}

fn fund(svm: &mut litesvm::LiteSVM, pk: &Pubkey, lamports: u64) {
    svm.airdrop(pk, lamports).unwrap();
}

fn send_tx(
    svm: &mut litesvm::LiteSVM,
    payer: &Keypair,
    ixs: Vec<Instruction>,
    signers: Vec<&Keypair>,
) -> TxResult {
    let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(10_000_000);
    let mut all_ixs = vec![compute_ix];
    all_ixs.extend(ixs);
    let msg = Message::new(&all_ixs, Some(&payer.pubkey()));
    let mut all_signers: Vec<&Keypair> = vec![payer];
    all_signers.extend(signers);
    let tx = Transaction::new(&all_signers, msg, svm.latest_blockhash());
    svm.send_transaction(tx)
}

struct Ctx {
    svm: litesvm::LiteSVM,
    pid: Pubkey,
    user: Keypair,
    provider: Keypair,
    channel_id: [u8; 32],
    channel_pda: Pubkey,
    escrow_pda: Pubkey,
    mint_pk: Pubkey,
    vault_a: Pubkey,
    vault_b: Pubkey,
    tree_depth: u32,
    initial_root: [u8; 32],
}

impl Ctx {
    fn new() -> Self {
        let (mut svm, pid) = setup_svm();
        let user = kp(1);
        let provider = kp(2);
        let mint_pk = kp(10).pubkey();

        fund(&mut svm, &user.pubkey(), 100_000_000_000);
        fund(&mut svm, &provider.pubkey(), 100_000_000_000);

        svm.set_account(mint_pk, make_mint_acct(&user.pubkey(), 9)).unwrap();

        let vault_a_pk = kp(20).pubkey();
        let vault_b_pk = kp(21).pubkey();
        svm.set_account(vault_a_pk, make_token_acct(&mint_pk, &user.pubkey(), 1_000_000_000_000)).unwrap();
        svm.set_account(vault_b_pk, make_token_acct(&mint_pk, &provider.pubkey(), 500_000_000_000)).unwrap();

        let channel_id: [u8; 32] = [0xAA; 32];
        let tree_depth: u32 = 3;
        let deposit_a: u64 = 100_000_000_000;
        let leaf0 = build_simple_leaf(&user.pubkey(), deposit_a);
        let initial_root = compute_merkle_root(&[hash(&leaf0).to_bytes()]);

        let (channel_pda, _) = Pubkey::find_program_address(&[b"channel", &channel_id], &pid);
        let (escrow_pda, _) = Pubkey::find_program_address(&[b"escrow", &channel_id], &pid);

        let mut ctx = Ctx { svm, pid, user, provider, channel_id, channel_pda, escrow_pda, mint_pk, vault_a: vault_a_pk, vault_b: vault_b_pk, tree_depth, initial_root };
        ctx.open_channel(0, 100, 10);
        ctx
    }

    fn open_channel(&mut self, open_slot: u64, challenge_duration: u64, min_challenge_delay: u64) {
        let deposit_a: u64 = 100_000_000_000;
        let mut msg = Vec::with_capacity(76);
        msg.extend_from_slice(&self.channel_id);
        msg.extend_from_slice(&deposit_a.to_le_bytes());
        msg.extend_from_slice(&self.tree_depth.to_le_bytes());
        msg.extend_from_slice(&self.initial_root);
        let sig_a = sign_ed25519(&msg, &self.user);

        let mut data = Vec::with_capacity(108);
        data.extend_from_slice(&ixDisc("open_channel"));
        data.extend_from_slice(&self.channel_id);
        data.extend_from_slice(&deposit_a.to_le_bytes());
        data.extend_from_slice(&self.tree_depth.to_le_bytes());
        data.extend_from_slice(&open_slot.to_le_bytes());
        data.extend_from_slice(&challenge_duration.to_le_bytes());
        data.extend_from_slice(&min_challenge_delay.to_le_bytes());
        data.extend_from_slice(&self.initial_root);

        let rent = Pubkey::from_str(RENT_SYSVAR).unwrap();
        let tp = Pubkey::from_str(TOKEN_PROG).unwrap();
        let sys = Pubkey::from_str(SYS_PROGRAM).unwrap();
        let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();

        let ed25519_ix = build_ed25519_ix(&self.user.pubkey(), &msg, &sig_a);

        let ix = Instruction { program_id: self.pid, accounts: vec![
            AccountMeta::new(self.channel_pda, false),
            AccountMeta::new_readonly(self.user.pubkey(), true),
            AccountMeta::new_readonly(self.user.pubkey(), false),
            AccountMeta::new_readonly(self.provider.pubkey(), false),
            AccountMeta::new_readonly(self.mint_pk, false),
            AccountMeta::new_readonly(self.vault_a, false),
            AccountMeta::new_readonly(self.vault_b, false),
            AccountMeta::new(self.user.pubkey(), true),
            AccountMeta::new_readonly(sys, false),
            AccountMeta::new_readonly(tp, false),
            AccountMeta::new_readonly(rent, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ], data };

        send_tx(&mut self.svm, &self.user, vec![ed25519_ix, ix], vec![]).unwrap_or_else(|e| panic!("open_channel failed: {:?}", e));
    }

    fn channel_status(&mut self) -> u8 {
        let acct = self.svm.get_account(&self.channel_pda).expect("channel account should exist");
        acct.data()[136]
    }

    fn channel_sequence(&mut self) -> u64 {
        let acct = self.svm.get_account(&self.channel_pda).unwrap();
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&acct.data()[137..145]);
        u64::from_le_bytes(buf)
    }
}

#[test]
fn test_open_channel_valid_signature() {
    let ctx = Ctx::new();
    let svm = ctx.svm;
    let acct = svm.get_account(&ctx.channel_pda).unwrap();
    assert!(acct.data().len() > 136);
    assert_eq!(acct.data()[136], 0, "status should be Open");
    println!("PASS: open_channel with valid ed25519 signature");
}

#[test]
fn test_trigger_challenge() {
    let mut ctx = Ctx::new();
    ctx.svm.warp_to_slot(20);
    let new_root = [0x22; 32];

    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&20u64.to_le_bytes()); msg.extend_from_slice(&new_root);
    let sig = sign_ed25519(&msg, &ctx.user);

    let mut data = Vec::with_capacity(8 + 32 + 8);
    data.extend_from_slice(&ixDisc("trigger_challenge")); data.extend_from_slice(&new_root);
    data.extend_from_slice(&1u64.to_le_bytes());

    let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
    let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
    let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);

    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(ctx.user.pubkey(), true),
        AccountMeta::new_readonly(clock, false),
        AccountMeta::new_readonly(instructions_sysvar, false),
    ], data };

    send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix, ix], vec![]).expect("trigger_challenge should succeed");
    assert_eq!(ctx.channel_status(), 1, "status should be Challenged");
    println!("PASS: trigger_challenge with valid ed25519 signature");
}

#[test]
fn test_cooperative_settle() {
    let mut ctx = Ctx::new();
    let current_root = ctx.initial_root;

    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&1u64.to_le_bytes()); msg.extend_from_slice(&current_root);
    let sig_a = sign_ed25519(&msg, &ctx.user);
    let sig_b = sign_ed25519(&msg, &ctx.provider);

    let mut data = Vec::with_capacity(8 + 8 + 32 + 8);
    data.extend_from_slice(&ixDisc("cooperative_settle")); data.extend_from_slice(&1u64.to_le_bytes());
    data.extend_from_slice(&current_root); data.extend_from_slice(&50u64.to_le_bytes());

    let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
    let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
    let ed25519_ix_a = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig_a);
    let ed25519_ix_b = build_ed25519_ix(&ctx.provider.pubkey(), &msg, &sig_b);

    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(clock, false),
        AccountMeta::new_readonly(instructions_sysvar, false),
    ], data };

    send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix_a, ed25519_ix_b, ix], vec![]).expect("cooperative_settle should succeed");
    assert_eq!(ctx.channel_status(), 2, "status should be Settling");
    println!("PASS: cooperative_settle with valid ed25519 signatures from both parties");
}

#[test]
fn test_submit_counter_state() {
    let mut ctx = Ctx::new();
    ctx.svm.warp_to_slot(20);
    let root_v1 = [0x22; 32];
    {
        let mut msg = Vec::with_capacity(72);
        msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&20u64.to_le_bytes()); msg.extend_from_slice(&root_v1);
        let sig = sign_ed25519(&msg, &ctx.user);
        let mut data = Vec::new(); data.extend_from_slice(&ixDisc("trigger_challenge"));
        data.extend_from_slice(&root_v1); data.extend_from_slice(&1u64.to_le_bytes());
        let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
        let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
        let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);
        let ix = Instruction { program_id: ctx.pid, accounts: vec![
            AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(ctx.user.pubkey(), true),
            AccountMeta::new_readonly(clock, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ], data };
        send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix, ix], vec![]).unwrap();
    }

    let root_v2 = [0x33; 32];
    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&2u64.to_le_bytes()); msg.extend_from_slice(&root_v2);
    let sig_a = sign_ed25519(&msg, &ctx.user); let sig_b = sign_ed25519(&msg, &ctx.provider);

    let mut data = Vec::with_capacity(8 + 8 + 32);
    data.extend_from_slice(&ixDisc("submit_counter_state")); data.extend_from_slice(&2u64.to_le_bytes());
    data.extend_from_slice(&root_v2);

    let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
    let ed25519_ix_a = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig_a);
    let ed25519_ix_b = build_ed25519_ix(&ctx.provider.pubkey(), &msg, &sig_b);

    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false),
        AccountMeta::new_readonly(instructions_sysvar, false),
    ], data };
    send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix_a, ed25519_ix_b, ix], vec![]).expect("submit_counter_state should succeed");
    assert_eq!(ctx.channel_sequence(), 2);
    println!("PASS: submit_counter_state with valid ed25519 signatures");
}

#[test]
fn test_settle_after_timeout() {
    let mut ctx = Ctx::new();
    ctx.svm.warp_to_slot(20);
    {
        let new_root = [0x22; 32];
        let mut msg = Vec::with_capacity(72);
        msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&20u64.to_le_bytes()); msg.extend_from_slice(&new_root);
        let sig = sign_ed25519(&msg, &ctx.user);
        let mut data = Vec::new(); data.extend_from_slice(&ixDisc("trigger_challenge"));
        data.extend_from_slice(&new_root); data.extend_from_slice(&1u64.to_le_bytes());
        let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
        let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
        let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);
        let ix = Instruction { program_id: ctx.pid, accounts: vec![
            AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(ctx.user.pubkey(), true),
            AccountMeta::new_readonly(clock, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ], data };
        send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix, ix], vec![]).unwrap();
    }

    ctx.svm.warp_to_slot(150);
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&ixDisc("settle_after_timeout")); data.extend_from_slice(&50u64.to_le_bytes());
    let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(clock, false),
    ], data };

    send_tx(&mut ctx.svm, &ctx.user, vec![ix], vec![]).expect("settle_after_timeout should succeed");
    assert_eq!(ctx.channel_status(), 2, "status should be Settling");
    println!("PASS: settle_after_timeout");
}

#[test]
fn test_challenge_not_elapsed() {
    let mut ctx = Ctx::new();
    let new_root = [0x22; 32];
    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id); msg.extend_from_slice(&0u64.to_le_bytes()); msg.extend_from_slice(&new_root);
    let sig = sign_ed25519(&msg, &ctx.user);
    let mut data = Vec::with_capacity(8 + 32 + 8);
    data.extend_from_slice(&ixDisc("trigger_challenge")); data.extend_from_slice(&new_root);
    data.extend_from_slice(&1u64.to_le_bytes());
    let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
    let instructions_sysvar = Pubkey::from_str(INSTRUCTIONS_SYSVAR).unwrap();
    let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);
    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(ctx.user.pubkey(), true),
        AccountMeta::new_readonly(clock, false),
        AccountMeta::new_readonly(instructions_sysvar, false),
    ], data };

    let result = send_tx(&mut ctx.svm, &ctx.user, vec![ed25519_ix, ix], vec![]);
    assert!(result.is_err(), "Should fail: min_challenge_delay not elapsed");
    assert_eq!(ctx.channel_status(), 0, "status should still be Open");
    println!("PASS: trigger_challenge correctly rejected before min_challenge_delay");
}

#[test]
fn test_settle_wrong_status() {
    let mut ctx = Ctx::new();
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&ixDisc("settle_after_timeout")); data.extend_from_slice(&50u64.to_le_bytes());
    let clock = Pubkey::from_str(CLOCK_SYSVAR).unwrap();
    let ix = Instruction { program_id: ctx.pid, accounts: vec![
        AccountMeta::new(ctx.channel_pda, false), AccountMeta::new_readonly(clock, false),
    ], data };

    let result = send_tx(&mut ctx.svm, &ctx.user, vec![ix], vec![]);
    assert!(result.is_err(), "Should fail: channel is Open");
    println!("PASS: settle_after_timeout correctly rejected on Open channel");
}

#[test]
fn test_minimal_program_load() {
    // Test that the program can at least be invoked with a bad instruction
    let (mut svm, pid) = setup_svm();
    let user = kp(1);
    fund(&mut svm, &user.pubkey(), 100_000_000_000);

    // Send a random instruction to the program (should fail with a program error, not InvalidProgramForExecution)
    let mut data = vec![0u8; 8]; // random discriminator
    let ix = Instruction { program_id: pid, accounts: vec![], data };

    let result = send_tx(&mut svm, &user, vec![ix], vec![]);
    eprintln!("DEBUG: result = {:?}", result);
    // The program should be executed (even if it fails), not InvalidProgramForExecution
    match result {
        Ok(_) => {},
        Err(meta) => {
            // Should NOT be InvalidProgramForExecution
            let err_str = format!("{:?}", meta.err);
            eprintln!("DEBUG: err = {}", err_str);
            assert!(!err_str.contains("InvalidProgramForExecution"), "Program should be executable, got: {}", err_str);
        }
    }
    println!("PASS: program is loadable and executable");
}

#[test]
fn test_ed25519_ix_in_transaction() {
    // Test if litesvm can handle an ed25519 instruction in a transaction
    let (mut svm, pid) = setup_svm();
    let user = kp(1);
    fund(&mut svm, &user.pubkey(), 100_000_000_000);

    let msg = b"test message";
    let sig = sign_ed25519(msg, &user);
    let ed25519_ix = build_ed25519_ix(&user.pubkey(), msg, &sig);

    // Just send the ed25519 instruction alone
    let result = send_tx(&mut svm, &user, vec![ed25519_ix], vec![]);
    eprintln!("DEBUG: ed25519 only result = {:?}", result);
    match &result {
        Ok(meta) => eprintln!("DEBUG: ed25519 only OK, cu = {}", meta.compute_units_consumed),
        Err(meta) => {
            let err_str = format!("{:?}", meta.err);
            eprintln!("DEBUG: ed25519 only err = {}", err_str);
        }
    }
}
