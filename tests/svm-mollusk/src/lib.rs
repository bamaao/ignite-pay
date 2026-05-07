#![allow(dead_code, unused_imports)]

use mollusk_svm::Mollusk;
use mollusk_svm::program::keyed_account_for_system_program;
use mollusk_svm::result::InstructionResult;
use solana_account::{Account, ReadableAccount, WritableAccount};
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_sdk_ids::{bpf_loader, system_program, sysvar};
use solana_sha256_hasher::{hash, hashv};
use solana_signer::Signer;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

const PROGRAM_ID_STR: &str = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe";
const TOKEN_PROG: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

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

/// Build a native ed25519 instruction with the standard Solana layout.
/// All data (signature, pubkey, message) is embedded within the instruction itself
/// (self-referencing with ix_index = u16::MAX).
fn build_ed25519_ix(public_key: &Pubkey, message: &[u8], signature: &[u8; 64]) -> Instruction {
    let ed25519_pid = Pubkey::from_str("Ed25519SigVerify1111111111111111111111111111").unwrap();
    let data_start: u16 = 16;
    let sig_offset = data_start;
    let pk_offset = sig_offset + 64;
    let msg_offset = pk_offset + 32;
    let msg_size = message.len() as u16;
    let self_ix_index = u16::MAX;
    let mut data = Vec::with_capacity(16 + 64 + 32 + message.len());
    data.push(1u8);                                    // num_signatures
    data.push(0u8);                                    // padding
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
    let mut data = vec![0u8; 82];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = decimals;
    data[45] = 1;
    Account {
        lamports: 2_000_000,
        data,
        owner: system_program::id(),
        executable: false,
        rent_epoch: 0,
    }
}

fn make_token_acct(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Account {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    Account {
        lamports: 2_000_000,
        data,
        owner: Pubkey::from_str(TOKEN_PROG).unwrap(),
        executable: false,
        rent_epoch: 0,
    }
}

fn setup_mollusk() -> Mollusk {
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
    std::env::set_var("SBF_OUT_DIR", &sbf_out);
    let mut mollusk = Mollusk::new(&pid, "ignite_pay_program");
    mollusk.compute_budget.compute_unit_limit = 20_000_000;

    // Register a noop program as the ed25519 precompile so that
    // ed25519 verification instructions can be included in instruction chains.
    let ed25519_pid = Pubkey::from_str("Ed25519SigVerify1111111111111111111111111111").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let noop_path = std::path::Path::new(&manifest_dir).join("noop_ed25519.so");
    let noop_bytes = std::fs::read(&noop_path)
        .expect("noop_ed25519.so not found in test directory");
    mollusk.add_program_with_loader_and_elf(&ed25519_pid, &bpf_loader::id(), &noop_bytes);

    mollusk
}

struct Ctx {
    mollusk: Mollusk,
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
    state: HashMap<Pubkey, Account>,
}

impl Ctx {
    fn new() -> Self {
        let pid = Pubkey::from_str(PROGRAM_ID_STR).unwrap();
        let mollusk = setup_mollusk();

        let user = kp(1);
        let provider = kp(2);
        let mint_pk = kp(10).pubkey();

        let state = Self::init_state(&user, &provider, &mint_pk);

        let channel_id: [u8; 32] = [0xAA; 32];
        let tree_depth: u32 = 3;
        let deposit_a: u64 = 100_000_000_000;
        let leaf0 = build_simple_leaf(&user.pubkey(), deposit_a);
        let initial_root = compute_merkle_root(&[hash(&leaf0).to_bytes()]);

        let (channel_pda, _) = Pubkey::find_program_address(&[b"channel", &channel_id], &pid);
        let (escrow_pda, _) = Pubkey::find_program_address(&[b"escrow", &channel_id], &pid);

        let vault_a_pk = kp(20).pubkey();
        let vault_b_pk = kp(21).pubkey();

        let mut ctx = Ctx {
            mollusk, pid, user, provider,
            channel_id, channel_pda, escrow_pda,
            mint_pk, vault_a: vault_a_pk, vault_b: vault_b_pk,
            tree_depth, initial_root, state,
        };

        ctx.open_channel(0, 100, 10);
        ctx
    }

    fn init_state(user: &Keypair, provider: &Keypair, mint_pk: &Pubkey) -> HashMap<Pubkey, Account> {
        let mut state = HashMap::new();
        let sys = system_program::id();
        state.insert(user.pubkey(), Account::new(100_000_000_000, 0, &sys));
        state.insert(provider.pubkey(), Account::new(100_000_000_000, 0, &sys));
        state.insert(*mint_pk, make_mint_acct(&user.pubkey(), 9));

        let vault_a_pk = kp(20).pubkey();
        let vault_b_pk = kp(21).pubkey();
        state.insert(vault_a_pk, make_token_acct(mint_pk, &user.pubkey(), 1_000_000_000_000));
        state.insert(vault_b_pk, make_token_acct(mint_pk, &provider.pubkey(), 500_000_000_000));

        state.insert(
            Pubkey::from_str(TOKEN_PROG).unwrap(),
            Account { lamports: 1, data: vec![], owner: Pubkey::default(), executable: true, rent_epoch: 0 },
        );

        // Rent sysvar must be provided with valid serialized data for Anchor validation.
        let rent_data = {
            let mut d = Vec::with_capacity(17);
            d.extend_from_slice(&1u64.to_le_bytes()); // lamports_per_byte_year
            d.extend_from_slice(&0.5f64.to_le_bytes()); // exemption_threshold
            d.push(50); // burn_percent
            d
        };
        state.insert(
            sysvar::rent::id(),
            Account { lamports: 1, data: rent_data, owner: Pubkey::default(), executable: false, rent_epoch: 0 },
        );

        // Clock sysvar is NOT provided — mollusk handles it internally via warp_to_slot.
        // If we provide a static clock account, it would override the warped slot value.
        state
    }

    fn build_accounts(&self, ix: &Instruction) -> Vec<(Pubkey, Account)> {
        // Skip the program itself (mollusk resolves from ELF cache) and the
        // instructions sysvar (mollusk auto-populates it from the instruction chain).
        let skip: HashSet<Pubkey> = [self.pid, sysvar::instructions::id()].into_iter().collect();

        let sys_acct = keyed_account_for_system_program();
        let clock_acct = self.mollusk.sysvars.keyed_account_for_clock_sysvar();

        let mut seen = HashSet::new();
        let mut accounts = Vec::new();
        for meta in &ix.accounts {
            if !seen.insert(meta.pubkey) { continue; }
            if skip.contains(&meta.pubkey) { continue; }
            if meta.pubkey == system_program::id() {
                accounts.push(sys_acct.clone());
            } else if meta.pubkey == sysvar::clock::id() {
                accounts.push(clock_acct.clone());
            } else if let Some(acct) = self.state.get(&meta.pubkey) {
                accounts.push((meta.pubkey, acct.clone()));
            } else {
                accounts.push((meta.pubkey, Account::default()));
            }
        }
        accounts
    }

    /// Send a single instruction (no ed25519 signatures).
    fn send_ix(&mut self, ix: Instruction) -> InstructionResult {
        let accounts = self.build_accounts(&ix);
        let result = self.mollusk.process_instruction(&ix, &accounts);

        // On success, update state from resulting accounts
        if result.program_result.is_ok() {
            let updated: Vec<(Pubkey, Account)> = result.resulting_accounts.clone();
            for (pk, acct) in updated {
                self.state.insert(pk, acct);
            }
        }

        result
    }

    /// Send an instruction chain: ed25519 verification instructions + program instruction.
    /// Mollusk auto-populates the instructions sysvar from the chain (PR #156).
    fn send_ix_chain(&mut self, chain: &[Instruction]) -> InstructionResult {
        // The last instruction is the program instruction; build accounts from it.
        let program_ix = &chain[chain.len() - 1];
        let accounts = self.build_accounts(program_ix);
        let result = self.mollusk.process_instruction_chain(chain, &accounts);

        // On success, update state from resulting accounts
        if result.program_result.is_ok() {
            let updated: Vec<(Pubkey, Account)> = result.resulting_accounts.clone();
            for (pk, acct) in updated {
                self.state.insert(pk, acct);
            }
        }

        result
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

        let tp = Pubkey::from_str(TOKEN_PROG).unwrap();
        let instructions_sysvar = sysvar::instructions::id();

        let ed25519_ix = build_ed25519_ix(&self.user.pubkey(), &msg, &sig_a);

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(self.channel_pda, false),
                AccountMeta::new_readonly(self.user.pubkey(), true),
                AccountMeta::new_readonly(self.user.pubkey(), false),
                AccountMeta::new_readonly(self.provider.pubkey(), false),
                AccountMeta::new_readonly(self.mint_pk, false),
                AccountMeta::new_readonly(self.vault_a, false),
                AccountMeta::new_readonly(self.vault_b, false),
                AccountMeta::new(self.user.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(tp, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(instructions_sysvar, false),
            ],
            data,
        };

        let result = self.send_ix_chain(&[ed25519_ix, ix]);
        if result.program_result.is_err() {
            panic!("open_channel failed: {:?}", result.raw_result);
        }
    }

    fn channel_status(&self) -> u8 {
        self.state.get(&self.channel_pda).expect("channel account should exist").data[136]
    }

    fn channel_sequence(&self) -> u64 {
        let acct = self.state.get(&self.channel_pda).unwrap();
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&acct.data[137..145]);
        u64::from_le_bytes(buf)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn test_open_channel_valid_signature() {
    let ctx = Ctx::new();
    assert!(ctx.state.contains_key(&ctx.channel_pda));
    let acct = ctx.state.get(&ctx.channel_pda).unwrap();
    assert!(acct.data.len() > 136);
    assert_eq!(acct.data[136], 0, "status should be Open");
    println!("PASS: open_channel with valid ed25519 signature");
}

#[test]
fn test_trigger_challenge() {
    let mut ctx = Ctx::new();
    ctx.mollusk.warp_to_slot(20);
    let new_root = [0x22; 32];

    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id);
    msg.extend_from_slice(&20u64.to_le_bytes());
    msg.extend_from_slice(&new_root);
    let sig = sign_ed25519(&msg, &ctx.user);

    let mut data = Vec::with_capacity(8 + 32 + 8);
    data.extend_from_slice(&ixDisc("trigger_challenge"));
    data.extend_from_slice(&new_root);
    data.extend_from_slice(&1u64.to_le_bytes());

    let clock = sysvar::clock::id();
    let instructions_sysvar = sysvar::instructions::id();
    let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);

    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(ctx.user.pubkey(), true),
            AccountMeta::new_readonly(clock, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ],
        data,
    };

    let result = ctx.send_ix_chain(&[ed25519_ix, ix]);
    assert!(result.program_result.is_ok(), "trigger_challenge should succeed");
    assert_eq!(ctx.channel_status(), 1, "status should be Challenged");
    println!("PASS: trigger_challenge with valid ed25519 signature");
}

#[test]
fn test_cooperative_settle() {
    let mut ctx = Ctx::new();
    let current_root = ctx.initial_root;

    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id);
    msg.extend_from_slice(&1u64.to_le_bytes());
    msg.extend_from_slice(&current_root);
    let sig_a = sign_ed25519(&msg, &ctx.user);
    let sig_b = sign_ed25519(&msg, &ctx.provider);

    let mut data = Vec::with_capacity(8 + 8 + 32 + 8);
    data.extend_from_slice(&ixDisc("cooperative_settle"));
    data.extend_from_slice(&1u64.to_le_bytes());
    data.extend_from_slice(&current_root);
    data.extend_from_slice(&50u64.to_le_bytes());

    let clock = sysvar::clock::id();
    let instructions_sysvar = sysvar::instructions::id();
    let ed25519_ix_a = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig_a);
    let ed25519_ix_b = build_ed25519_ix(&ctx.provider.pubkey(), &msg, &sig_b);

    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(clock, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ],
        data,
    };

    let result = ctx.send_ix_chain(&[ed25519_ix_a, ed25519_ix_b, ix]);
    assert!(result.program_result.is_ok(), "cooperative_settle should succeed");
    assert_eq!(ctx.channel_status(), 2, "status should be Settling");
    println!("PASS: cooperative_settle with valid ed25519 signatures from both parties");
}

#[test]
fn test_submit_counter_state() {
    let mut ctx = Ctx::new();
    ctx.mollusk.warp_to_slot(20);
    let root_v1 = [0x22; 32];

    // First trigger challenge
    {
        let mut msg = Vec::with_capacity(72);
        msg.extend_from_slice(&ctx.channel_id);
        msg.extend_from_slice(&20u64.to_le_bytes());
        msg.extend_from_slice(&root_v1);
        let sig = sign_ed25519(&msg, &ctx.user);

        let mut data = Vec::new();
        data.extend_from_slice(&ixDisc("trigger_challenge"));
        data.extend_from_slice(&root_v1);
        data.extend_from_slice(&1u64.to_le_bytes());

        let clock = sysvar::clock::id();
        let instructions_sysvar = sysvar::instructions::id();
        let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);

        let ix = Instruction {
            program_id: ctx.pid,
            accounts: vec![
                AccountMeta::new(ctx.channel_pda, false),
                AccountMeta::new_readonly(ctx.user.pubkey(), true),
                AccountMeta::new_readonly(clock, false),
                AccountMeta::new_readonly(instructions_sysvar, false),
            ],
            data,
        };
        let result = ctx.send_ix_chain(&[ed25519_ix, ix]);
        assert!(result.program_result.is_ok());
    }

    // Then submit counter state
    let root_v2 = [0x33; 32];
    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id);
    msg.extend_from_slice(&2u64.to_le_bytes());
    msg.extend_from_slice(&root_v2);
    let sig_a = sign_ed25519(&msg, &ctx.user);
    let sig_b = sign_ed25519(&msg, &ctx.provider);

    let mut data = Vec::with_capacity(8 + 8 + 32);
    data.extend_from_slice(&ixDisc("submit_counter_state"));
    data.extend_from_slice(&2u64.to_le_bytes());
    data.extend_from_slice(&root_v2);

    let instructions_sysvar = sysvar::instructions::id();
    let ed25519_ix_a = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig_a);
    let ed25519_ix_b = build_ed25519_ix(&ctx.provider.pubkey(), &msg, &sig_b);

    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ],
        data,
    };

    let result = ctx.send_ix_chain(&[ed25519_ix_a, ed25519_ix_b, ix]);
    assert!(result.program_result.is_ok(), "submit_counter_state should succeed");
    assert_eq!(ctx.channel_sequence(), 2);
    println!("PASS: submit_counter_state with valid ed25519 signatures");
}

#[test]
fn test_settle_after_timeout() {
    let mut ctx = Ctx::new();
    ctx.mollusk.warp_to_slot(20);

    // Trigger challenge
    {
        let new_root = [0x22; 32];
        let mut msg = Vec::with_capacity(72);
        msg.extend_from_slice(&ctx.channel_id);
        msg.extend_from_slice(&20u64.to_le_bytes());
        msg.extend_from_slice(&new_root);
        let sig = sign_ed25519(&msg, &ctx.user);

        let mut data = Vec::new();
        data.extend_from_slice(&ixDisc("trigger_challenge"));
        data.extend_from_slice(&new_root);
        data.extend_from_slice(&1u64.to_le_bytes());

        let clock = sysvar::clock::id();
        let instructions_sysvar = sysvar::instructions::id();
        let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);

        let ix = Instruction {
            program_id: ctx.pid,
            accounts: vec![
                AccountMeta::new(ctx.channel_pda, false),
                AccountMeta::new_readonly(ctx.user.pubkey(), true),
                AccountMeta::new_readonly(clock, false),
                AccountMeta::new_readonly(instructions_sysvar, false),
            ],
            data,
        };
        ctx.send_ix_chain(&[ed25519_ix, ix]);
    }

    // Warp past challenge duration and settle
    ctx.mollusk.warp_to_slot(150);

    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&ixDisc("settle_after_timeout"));
    data.extend_from_slice(&50u64.to_le_bytes());

    let clock = sysvar::clock::id();
    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(clock, false),
        ],
        data,
    };

    let result = ctx.send_ix(ix);
    assert!(result.program_result.is_ok(), "settle_after_timeout should succeed");
    assert_eq!(ctx.channel_status(), 2, "status should be Settling");
    println!("PASS: settle_after_timeout");
}

#[test]
fn test_challenge_not_elapsed() {
    let mut ctx = Ctx::new();
    let new_root = [0x22; 32];

    let mut msg = Vec::with_capacity(72);
    msg.extend_from_slice(&ctx.channel_id);
    msg.extend_from_slice(&0u64.to_le_bytes());
    msg.extend_from_slice(&new_root);
    let sig = sign_ed25519(&msg, &ctx.user);

    let mut data = Vec::with_capacity(8 + 32 + 8);
    data.extend_from_slice(&ixDisc("trigger_challenge"));
    data.extend_from_slice(&new_root);
    data.extend_from_slice(&1u64.to_le_bytes());

    let clock = sysvar::clock::id();
    let instructions_sysvar = sysvar::instructions::id();
    let ed25519_ix = build_ed25519_ix(&ctx.user.pubkey(), &msg, &sig);

    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(ctx.user.pubkey(), true),
            AccountMeta::new_readonly(clock, false),
            AccountMeta::new_readonly(instructions_sysvar, false),
        ],
        data,
    };

    let result = ctx.send_ix_chain(&[ed25519_ix, ix]);
    assert!(result.program_result.is_err(), "Should fail: min_challenge_delay not elapsed");
    assert_eq!(ctx.channel_status(), 0, "status should still be Open");
    println!("PASS: trigger_challenge correctly rejected before min_challenge_delay");
}

#[test]
fn test_settle_wrong_status() {
    let mut ctx = Ctx::new();

    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&ixDisc("settle_after_timeout"));
    data.extend_from_slice(&50u64.to_le_bytes());

    let clock = sysvar::clock::id();
    let ix = Instruction {
        program_id: ctx.pid,
        accounts: vec![
            AccountMeta::new(ctx.channel_pda, false),
            AccountMeta::new_readonly(clock, false),
        ],
        data,
    };

    let result = ctx.send_ix(ix);
    assert!(result.program_result.is_err(), "Should fail: channel is Open");
    println!("PASS: settle_after_timeout correctly rejected on Open channel");
}
