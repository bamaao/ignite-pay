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

use mollusk_svm::Mollusk;
use mollusk_svm::program::keyed_account_for_system_program;
use mollusk_svm::result::InstructionResult;
use solana_account::{Account, ReadableAccount, WritableAccount};
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_sdk_ids::system_program;
use solana_sha256_hasher::hash;
use solana_signer::Signer;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

const PROGRAM_ID_STR: &str = "D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D";

fn ixDisc(name: &str) -> [u8; 8] {
    let h = hash(format!("global:{}", name).as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

fn kp(seed: u8) -> Keypair {
    let mut s = [0u8; 32];
    s[0] = seed;
    Keypair::new_from_array(s)
}

fn sign_ed25519(message: &[u8], keypair: &Keypair) -> [u8; 64] {
    *keypair.sign_message(message).as_array()
}

fn setup_mollusk() -> Mollusk {
    let pid = Pubkey::from_str(PROGRAM_ID_STR).unwrap();
    let sbf_out = std::env::var("SBF_OUT_DIR")
        .unwrap_or_else(|_| "target/deploy".to_string());
    let sbf_out = if std::path::Path::new(&sbf_out).is_absolute() {
        sbf_out
    } else {
        // Resolve relative to project root (two levels up from this crate)
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set");
        let project_root = std::path::Path::new(&manifest_dir)
            .parent().unwrap()  // tests/
            .parent().unwrap(); // project root
        project_root.join(&sbf_out).to_str().unwrap().to_string()
    };
    std::env::set_var("SBF_OUT_DIR", &sbf_out);
    let mut mollusk = Mollusk::new(&pid, "ignite_pay_did_program");
    mollusk.compute_budget.compute_unit_limit = 10_000_000;
    mollusk
}

// ─── MerchantDidAccount layout offsets (153 bytes total) ──────────────────
const OFF_ORIGINAL_PK: usize = 8;
const OFF_CONTROLLER_PK: usize = 40;
const OFF_RECOVERY_PK: usize = 72;
const OFF_VC_HASH: usize = 104;
const OFF_NONCE: usize = 144;

// ─── PlatformConfig layout offsets (73 bytes total) ───────────────────────
const PC_OFF_PLATFORM_PK: usize = 8;
const PC_OFF_AUTHORITY: usize = 40;

struct Ctx {
    mollusk: Mollusk,
    pid: Pubkey,
    platform_keypair: Keypair,
    merchant_keypair: Keypair,
    recovery_keypair: Keypair,
    platform_config_pda: Pubkey,
    merchant_did_pda: Pubkey,
    state: HashMap<Pubkey, Account>,
}

impl Ctx {
    fn new() -> Self {
        let pid = Pubkey::from_str(PROGRAM_ID_STR).unwrap();
        let mollusk = setup_mollusk();

        let platform_keypair = kp(1);
        let merchant_keypair = kp(2);
        let recovery_keypair = kp(3);

        let (platform_config_pda, _) =
            Pubkey::find_program_address(&[b"platform-config"], &pid);
        let (merchant_did_pda, _) = Pubkey::find_program_address(
            &[b"merchant-did", merchant_keypair.pubkey().as_ref()],
            &pid,
        );

        let mut state = HashMap::new();
        let sys = system_program::id();
        state.insert(platform_keypair.pubkey(), Account::new(100_000_000_000, 0, &sys));
        state.insert(merchant_keypair.pubkey(), Account::new(100_000_000_000, 0, &sys));
        state.insert(recovery_keypair.pubkey(), Account::new(100_000_000_000, 0, &sys));

        let mut ctx = Ctx {
            mollusk, pid, platform_keypair, merchant_keypair, recovery_keypair,
            platform_config_pda, merchant_did_pda, state,
        };

        ctx.init_platform();
        ctx
    }

    fn build_accounts(&self, ix: &Instruction) -> Vec<(Pubkey, Account)> {
        let skip: HashSet<Pubkey> = [self.pid].into_iter().collect();
        let sys_acct = keyed_account_for_system_program();

        let mut seen = HashSet::new();
        let mut accounts = Vec::new();
        for meta in &ix.accounts {
            if !seen.insert(meta.pubkey) { continue; }
            if skip.contains(&meta.pubkey) { continue; }
            if meta.pubkey == system_program::id() {
                accounts.push(sys_acct.clone());
            } else if let Some(acct) = self.state.get(&meta.pubkey) {
                accounts.push((meta.pubkey, acct.clone()));
            } else {
                accounts.push((meta.pubkey, Account::default()));
            }
        }
        accounts
    }

    fn send_ix(&mut self, ix: Instruction) -> InstructionResult {
        let accounts = self.build_accounts(&ix);
        let result = self.mollusk.process_instruction(&ix, &accounts);
        if result.program_result.is_ok() {
            for (pk, acct) in result.resulting_accounts.clone() {
                self.state.insert(pk, acct);
            }
        }
        result
    }

    fn init_platform(&mut self) {
        let pk_bytes = self.platform_keypair.pubkey().to_bytes();
        let disc = ixDisc("init_platform");
        let mut data = Vec::with_capacity(8 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&pk_bytes);

        let signer_pk = self.platform_keypair.pubkey();
        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(signer_pk, true),
                AccountMeta::new(self.platform_config_pda, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        };

        let result = self.send_ix(ix);
        if result.program_result.is_err() {
            panic!("init_platform failed: {:?}", result.raw_result);
        }
    }

    fn initialize_did(&mut self, vc_hash: [u8; 32], platform_sig: [u8; 64]) -> InstructionResult {
        let disc = ixDisc("initialize_did");
        let merchant_pk = self.merchant_keypair.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 64 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&platform_sig);
        data.extend_from_slice(merchant_pk.as_ref());

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(merchant_pk, true),
                AccountMeta::new(self.merchant_did_pda, false),
                AccountMeta::new_readonly(self.platform_config_pda, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        };

        self.send_ix(ix)
    }

    fn update_did_with_vc(
        &mut self,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_sig: [u8; 64],
        signer: Keypair,
    ) -> InstructionResult {
        let disc = ixDisc("update_did_with_vc");
        let signer_pk = signer.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 8 + 64 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&nonce.to_le_bytes());
        data.extend_from_slice(&platform_sig);
        data.extend_from_slice(signer_pk.as_ref());

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(signer_pk, true),
                AccountMeta::new(self.merchant_did_pda, false),
                AccountMeta::new_readonly(self.platform_config_pda, false),
            ],
            data,
        };

        self.send_ix(ix)
    }

    fn set_recovery_key(
        &mut self,
        recovery_pk: Pubkey,
        nonce: u64,
        signer: Keypair,
    ) -> InstructionResult {
        let disc = ixDisc("set_recovery_key");
        let signer_pk = signer.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 8);
        data.extend_from_slice(&disc);
        data.extend_from_slice(recovery_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(signer_pk, true),
                AccountMeta::new(self.merchant_did_pda, false),
            ],
            data,
        };

        self.send_ix(ix)
    }

    fn recover_controller(
        &mut self,
        new_controller_pk: Pubkey,
        nonce: u64,
        signer: Keypair,
    ) -> InstructionResult {
        let disc = ixDisc("recover_controller");
        let signer_pk = signer.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 8);
        data.extend_from_slice(&disc);
        data.extend_from_slice(new_controller_pk.as_ref());
        data.extend_from_slice(&nonce.to_le_bytes());

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(signer_pk, true),
                AccountMeta::new(self.merchant_did_pda, false),
            ],
            data,
        };

        self.send_ix(ix)
    }

    fn revoke_vc(
        &mut self,
        vc_hash: [u8; 32],
        credential_subject_pk: Pubkey,
        reason: u8,
        authority: Keypair,
        revoked_vc_pda: Pubkey,
    ) -> InstructionResult {
        let disc = ixDisc("revoke_vc");
        let auth_pk = authority.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 32 + 1);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(credential_subject_pk.as_ref());
        data.push(reason);

        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(auth_pk, true),
                AccountMeta::new_readonly(self.platform_config_pda, false),
                AccountMeta::new(revoked_vc_pda, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        };

        self.send_ix(ix)
    }

    fn make_platform_signature(&self, vc_hash: &[u8; 32], subject_pk: &Pubkey) -> [u8; 64] {
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(subject_pk.as_ref());
        message.extend_from_slice(vc_hash);
        sign_ed25519(&message, &self.platform_keypair)
    }

    fn did_nonce(&self) -> u64 {
        let acct = self.state.get(&self.merchant_did_pda).unwrap();
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&acct.data[OFF_NONCE..OFF_NONCE + 8]);
        u64::from_le_bytes(buf)
    }

    fn did_vc_hash(&self) -> [u8; 32] {
        let acct = self.state.get(&self.merchant_did_pda).unwrap();
        let mut h = [0u8; 32];
        h.copy_from_slice(&acct.data[OFF_VC_HASH..OFF_VC_HASH + 32]);
        h
    }

    fn did_controller_pk(&self) -> Pubkey {
        let acct = self.state.get(&self.merchant_did_pda).unwrap();
        Pubkey::try_from(&acct.data[OFF_CONTROLLER_PK..OFF_CONTROLLER_PK + 32]).unwrap()
    }

    fn did_recovery_pk(&self) -> Pubkey {
        let acct = self.state.get(&self.merchant_did_pda).unwrap();
        Pubkey::try_from(&acct.data[OFF_RECOVERY_PK..OFF_RECOVERY_PK + 32]).unwrap()
    }
}

// ─── Happy path tests ─────────────────────────────────────────────────────

#[test]
fn test_init_platform() {
    let ctx = Ctx::new();
    let acct = ctx.state.get(&ctx.platform_config_pda).unwrap();
    assert_eq!(
        &acct.data[PC_OFF_PLATFORM_PK..PC_OFF_PLATFORM_PK + 32],
        ctx.platform_keypair.pubkey().as_ref(),
        "platform_ed25519_pubkey should match"
    );
    assert_eq!(
        &acct.data[PC_OFF_AUTHORITY..PC_OFF_AUTHORITY + 32],
        ctx.platform_keypair.pubkey().as_ref(),
        "authority should match"
    );
    println!("PASS: init_platform");
}

#[test]
fn test_initialize_did() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());

    let result = ctx.initialize_did(vc_hash, sig);
    assert!(result.program_result.is_ok(), "initialize_did should succeed: {:?}", result.raw_result);

    let acct = ctx.state.get(&ctx.merchant_did_pda).unwrap();
    assert_eq!(acct.data.len(), 153, "DID account should be 153 bytes");
    assert_eq!(
        &acct.data[OFF_ORIGINAL_PK..OFF_ORIGINAL_PK + 32],
        ctx.merchant_keypair.pubkey().as_ref(),
        "original_pk should match merchant"
    );
    assert_eq!(
        &acct.data[OFF_CONTROLLER_PK..OFF_CONTROLLER_PK + 32],
        ctx.merchant_keypair.pubkey().as_ref(),
        "controller_pk should match merchant"
    );
    assert_eq!(&acct.data[OFF_VC_HASH..OFF_VC_HASH + 32], &vc_hash, "vc_hash should match");
    assert_eq!(ctx.did_nonce(), 0, "initial nonce should be 0");
    println!("PASS: initialize_did");
}

#[test]
fn test_update_did_with_vc() {
    let mut ctx = Ctx::new();
    let vc_hash_1 = [0xAA; 32];
    let sig1 = ctx.make_platform_signature(&vc_hash_1, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash_1, sig1).raw_result.unwrap();

    let vc_hash_2 = [0xBB; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 0, sig2, merchant);
    assert!(result.program_result.is_ok(), "update_did_with_vc should succeed: {:?}", result.raw_result);

    assert_eq!(ctx.did_vc_hash(), vc_hash_2, "vc_hash should be updated");
    assert_eq!(ctx.did_nonce(), 1, "nonce should increment to 1");
    println!("PASS: update_did_with_vc");
}

#[test]
fn test_set_recovery_key() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    let result = ctx.set_recovery_key(recovery_pk, 0, merchant);
    assert!(result.program_result.is_ok(), "set_recovery_key should succeed: {:?}", result.raw_result);

    assert_eq!(ctx.did_recovery_pk(), ctx.recovery_keypair.pubkey(), "recovery_pk should match");
    assert_eq!(ctx.did_nonce(), 1, "nonce should increment to 1");
    println!("PASS: set_recovery_key");
}

#[test]
fn test_recover_controller() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 0, merchant).raw_result.unwrap();

    let new_controller = kp(4);
    let recovery = kp(3);
    let result = ctx.recover_controller(new_controller.pubkey(), 1, recovery);
    assert!(result.program_result.is_ok(), "recover_controller should succeed: {:?}", result.raw_result);

    assert_eq!(ctx.did_controller_pk(), new_controller.pubkey(), "controller should be updated");
    assert_eq!(ctx.did_nonce(), 2, "nonce should increment to 2");
    println!("PASS: recover_controller");
}

#[test]
fn test_revoke_vc() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash], &ctx.pid);

    let merchant_pk = ctx.merchant_keypair.pubkey();
    let platform = kp(1);
    let result = ctx.revoke_vc(vc_hash, merchant_pk, 1, platform, revoked_vc_pda);
    assert!(result.program_result.is_ok(), "revoke_vc should succeed: {:?}", result.raw_result);

    let acct = ctx.state.get(&revoked_vc_pda).expect("revoked_vc PDA should exist");
    assert_eq!(acct.data.len(), 114, "RevokedVc should be 114 bytes");
    println!("PASS: revoke_vc");
}

#[test]
fn test_full_lifecycle() {
    let mut ctx = Ctx::new();

    // 1. init_platform (done in Ctx::new)

    // 2. initialize_did
    let vc_hash_1 = [0x11; 32];
    let sig1 = ctx.make_platform_signature(&vc_hash_1, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash_1, sig1).raw_result.unwrap();
    assert_eq!(ctx.did_nonce(), 0);
    assert_eq!(ctx.did_vc_hash(), vc_hash_1);

    // 3. update_did_with_vc (nonce 0 -> 1)
    let vc_hash_2 = [0x22; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    ctx.update_did_with_vc(vc_hash_2, 0, sig2, merchant).raw_result.unwrap();
    assert_eq!(ctx.did_nonce(), 1);
    assert_eq!(ctx.did_vc_hash(), vc_hash_2);

    // 4. set_recovery_key (nonce 1 -> 2)
    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 1, merchant).raw_result.unwrap();
    assert_eq!(ctx.did_nonce(), 2);
    assert_eq!(ctx.did_recovery_pk(), ctx.recovery_keypair.pubkey());

    // 5. recover_controller (nonce 2 -> 3)
    let new_controller = kp(4);
    let recovery = kp(3);
    ctx.recover_controller(new_controller.pubkey(), 2, recovery).raw_result.unwrap();
    assert_eq!(ctx.did_nonce(), 3);
    assert_eq!(ctx.did_controller_pk(), new_controller.pubkey());

    // 6. update_did_with_vc as new controller (nonce 3 -> 4)
    let vc_hash_3 = [0x33; 32];
    let sig3 = ctx.make_platform_signature(&vc_hash_3, &new_controller.pubkey());
    ctx.update_did_with_vc(vc_hash_3, 3, sig3, new_controller).raw_result.unwrap();
    assert_eq!(ctx.did_nonce(), 4);
    assert_eq!(ctx.did_vc_hash(), vc_hash_3);

    // 7. revoke_vc
    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash_3], &ctx.pid);
    let merchant_pk = ctx.merchant_keypair.pubkey();
    let platform = kp(1);
    ctx.revoke_vc(vc_hash_3, merchant_pk, 2, platform, revoked_vc_pda).raw_result.unwrap();
    assert!(ctx.state.contains_key(&revoked_vc_pda));

    println!("PASS: full_lifecycle (7 steps, all assertions passed)");
}

// ─── Negative tests ───────────────────────────────────────────────────────

#[test]
fn test_initialize_did_wrong_signature() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let merchant_pk = ctx.merchant_keypair.pubkey();
    let mut message = Vec::with_capacity(64);
    message.extend_from_slice(merchant_pk.as_ref());
    message.extend_from_slice(&vc_hash);
    let wrong_sig = sign_ed25519(&message, &ctx.merchant_keypair);

    let result = ctx.initialize_did(vc_hash, wrong_sig);
    assert!(result.program_result.is_err(), "Should fail with wrong platform signature");
    println!("PASS: initialize_did_wrong_signature");
}

#[test]
fn test_update_did_wrong_nonce() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let vc_hash_2 = [0xBB; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 1, sig2, merchant);
    assert!(result.program_result.is_err(), "Should fail with wrong nonce");
    println!("PASS: update_did_wrong_nonce");
}

#[test]
fn test_update_did_wrong_controller() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let impostor = kp(99);
    let sys = system_program::id();
    ctx.state.insert(impostor.pubkey(), Account::new(100_000_000_000, 0, &sys));

    let vc_hash_2 = [0xBB; 32];
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &impostor.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 0, sig2, impostor);
    assert!(result.program_result.is_err(), "Should fail: signer is not controller");
    println!("PASS: update_did_wrong_controller");
}

#[test]
fn test_recover_controller_wrong_key() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 0, merchant).raw_result.unwrap();

    let new_controller = kp(4);
    let merchant = kp(2);
    let result = ctx.recover_controller(new_controller.pubkey(), 1, merchant);
    assert!(result.program_result.is_err(), "Should fail: wrong recovery key");
    println!("PASS: recover_controller_wrong_key");
}

#[test]
fn test_revoke_vc_unauthorized() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).raw_result.unwrap();

    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash], &ctx.pid);

    let impostor = kp(99);
    let sys = system_program::id();
    ctx.state.insert(impostor.pubkey(), Account::new(100_000_000_000, 0, &sys));

    let merchant_pk = ctx.merchant_keypair.pubkey();
    let result = ctx.revoke_vc(vc_hash, merchant_pk, 1, impostor, revoked_vc_pda);
    assert!(result.program_result.is_err(), "Should fail: unauthorized revocation");
    println!("PASS: revoke_vc_unauthorized");
}
