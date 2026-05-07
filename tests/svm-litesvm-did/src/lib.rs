#![allow(dead_code, unused_imports)]

use solana_account::{Account, ReadableAccount, WritableAccount};
use solana_address::Address;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_sha256_hasher::hash;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::str::FromStr;

type Pubkey = Address;
type TxResult = litesvm::types::TransactionResult;

const PROGRAM_ID_STR: &str = "D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D";
const SYS_PROGRAM: &str = "11111111111111111111111111111111";

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

fn setup_svm() -> (litesvm::LiteSVM, Pubkey) {
    let mut budget = solana_compute_budget::compute_budget::ComputeBudget::new_with_defaults(false, false);
    budget.compute_unit_limit = 10_000_000;
    let mut svm = litesvm::LiteSVM::new().with_compute_budget(budget);
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
    let so_path = format!("{}/ignite_pay_did_program.so", sbf_out);
    let bytes = std::fs::read(&so_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {} — run 'make build-sbf' or set SBF_OUT_DIR", so_path, e));
    let _ = svm.add_program(pid, &bytes);
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
    // Prepend a ComputeBudget instruction to request more CUs
    let compute_budget_ix = Instruction {
        program_id: Pubkey::from_str("ComputeBudget111111111111111111111111111111").unwrap(),
        accounts: vec![],
        data: {
            // SetComputeUnitLimit: discriminator 2 + u32 limit
            let mut d = Vec::with_capacity(5);
            d.push(2u8); // SetComputeUnitLimit discriminator
            d.extend_from_slice(&10_000_000u32.to_le_bytes());
            d
        },
    };
    let mut all_ixs = vec![compute_budget_ix];
    all_ixs.extend(ixs);

    let msg = Message::new(&all_ixs, Some(&payer.pubkey()));
    let mut all_signers: Vec<&Keypair> = vec![payer];
    all_signers.extend(signers);
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new(&all_signers, msg, blockhash);
    svm.send_transaction(tx)
}

const OFF_ORIGINAL_PK: usize = 8;
const OFF_CONTROLLER_PK: usize = 40;
const OFF_RECOVERY_PK: usize = 72;
const OFF_VC_HASH: usize = 104;
const OFF_NONCE: usize = 144;

const PC_OFF_PLATFORM_PK: usize = 8;
const PC_OFF_AUTHORITY: usize = 40;

struct Ctx {
    svm: litesvm::LiteSVM,
    pid: Pubkey,
    platform_keypair: Keypair,
    merchant_keypair: Keypair,
    recovery_keypair: Keypair,
    platform_config_pda: Pubkey,
    merchant_did_pda: Pubkey,
}

impl Ctx {
    fn new() -> Self {
        let (mut svm, pid) = setup_svm();
        let platform_keypair = kp(1);
        let merchant_keypair = kp(2);
        let recovery_keypair = kp(3);

        fund(&mut svm, &platform_keypair.pubkey(), 100_000_000_000);
        fund(&mut svm, &merchant_keypair.pubkey(), 100_000_000_000);
        fund(&mut svm, &recovery_keypair.pubkey(), 100_000_000_000);

        let (platform_config_pda, _) =
            Pubkey::find_program_address(&[b"platform-config"], &pid);
        let (merchant_did_pda, _) = Pubkey::find_program_address(
            &[b"merchant-did", merchant_keypair.pubkey().as_ref()],
            &pid,
        );

        let mut ctx = Ctx {
            svm, pid,
            platform_keypair, merchant_keypair, recovery_keypair,
            platform_config_pda, merchant_did_pda,
        };
        ctx.init_platform();
        ctx
    }

    fn init_platform(&mut self) {
        let pk_bytes = self.platform_keypair.pubkey().to_bytes();
        let disc = ixDisc("init_platform");
        let mut data = Vec::with_capacity(8 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&pk_bytes);

        let signer_pk = self.platform_keypair.pubkey();
        let sys = Pubkey::from_str(SYS_PROGRAM).unwrap();
        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(signer_pk, true),
                AccountMeta::new(self.platform_config_pda, false),
                AccountMeta::new_readonly(sys, false),
            ],
            data,
        };

        let result = send_tx(&mut self.svm, &self.platform_keypair, vec![ix], vec![]);
        result.unwrap();
    }

    fn initialize_did(&mut self, vc_hash: [u8; 32], platform_sig: [u8; 64]) -> TxResult {
        let disc = ixDisc("initialize_did");
        let merchant_pk = self.merchant_keypair.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 64 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(&platform_sig);
        data.extend_from_slice(merchant_pk.as_ref());

        let sys = Pubkey::from_str(SYS_PROGRAM).unwrap();
        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(merchant_pk, true),
                AccountMeta::new(self.merchant_did_pda, false),
                AccountMeta::new_readonly(self.platform_config_pda, false),
                AccountMeta::new_readonly(sys, false),
            ],
            data,
        };

        send_tx(&mut self.svm, &self.merchant_keypair, vec![ix], vec![])
    }

    fn update_did_with_vc(
        &mut self,
        vc_hash: [u8; 32],
        nonce: u64,
        platform_sig: [u8; 64],
        signer: &Keypair,
    ) -> TxResult {
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

        send_tx(&mut self.svm, signer, vec![ix], vec![])
    }

    fn set_recovery_key(
        &mut self,
        recovery_pk: Pubkey,
        nonce: u64,
        signer: &Keypair,
    ) -> TxResult {
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

        send_tx(&mut self.svm, signer, vec![ix], vec![])
    }

    fn recover_controller(
        &mut self,
        new_controller_pk: Pubkey,
        nonce: u64,
        signer: &Keypair,
    ) -> TxResult {
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

        send_tx(&mut self.svm, signer, vec![ix], vec![])
    }

    fn revoke_vc(
        &mut self,
        vc_hash: [u8; 32],
        credential_subject_pk: Pubkey,
        reason: u8,
        authority: &Keypair,
        revoked_vc_pda: Pubkey,
    ) -> TxResult {
        let disc = ixDisc("revoke_vc");
        let auth_pk = authority.pubkey();
        let mut data = Vec::with_capacity(8 + 32 + 32 + 1);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&vc_hash);
        data.extend_from_slice(credential_subject_pk.as_ref());
        data.push(reason);

        let sys = Pubkey::from_str(SYS_PROGRAM).unwrap();
        let ix = Instruction {
            program_id: self.pid,
            accounts: vec![
                AccountMeta::new(auth_pk, true),
                AccountMeta::new_readonly(self.platform_config_pda, false),
                AccountMeta::new(revoked_vc_pda, false),
                AccountMeta::new_readonly(sys, false),
            ],
            data,
        };

        send_tx(&mut self.svm, authority, vec![ix], vec![])
    }

    fn make_platform_signature(&self, vc_hash: &[u8; 32], subject_pk: &Pubkey) -> [u8; 64] {
        let mut message = Vec::with_capacity(64);
        message.extend_from_slice(subject_pk.as_ref());
        message.extend_from_slice(vc_hash);
        sign_ed25519(&message, &self.platform_keypair)
    }

    fn did_nonce(&mut self) -> u64 {
        let acct = self.svm.get_account(&self.merchant_did_pda).expect("did account should exist");
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&acct.data()[OFF_NONCE..OFF_NONCE + 8]);
        u64::from_le_bytes(buf)
    }

    fn did_vc_hash(&mut self) -> [u8; 32] {
        let acct = self.svm.get_account(&self.merchant_did_pda).expect("did account should exist");
        let mut h = [0u8; 32];
        h.copy_from_slice(&acct.data()[OFF_VC_HASH..OFF_VC_HASH + 32]);
        h
    }

    fn did_controller_pk(&mut self) -> Pubkey {
        let acct = self.svm.get_account(&self.merchant_did_pda).expect("did account should exist");
        Pubkey::try_from(&acct.data()[OFF_CONTROLLER_PK..OFF_CONTROLLER_PK + 32]).unwrap()
    }

    fn did_recovery_pk(&mut self) -> Pubkey {
        let acct = self.svm.get_account(&self.merchant_did_pda).expect("did account should exist");
        Pubkey::try_from(&acct.data()[OFF_RECOVERY_PK..OFF_RECOVERY_PK + 32]).unwrap()
    }
}

// ─── Happy path tests ─────────────────────────────────────────────────────

#[test]
fn test_init_platform() {
    let ctx = Ctx::new();
    let acct = ctx.svm.get_account(&ctx.platform_config_pda).unwrap();
    assert_eq!(
        &acct.data()[PC_OFF_PLATFORM_PK..PC_OFF_PLATFORM_PK + 32],
        ctx.platform_keypair.pubkey().as_ref(),
        "platform_ed25519_pubkey should match"
    );
    assert_eq!(
        &acct.data()[PC_OFF_AUTHORITY..PC_OFF_AUTHORITY + 32],
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
    assert!(result.is_ok(), "initialize_did should succeed: {:?}", result);

    let acct = ctx.svm.get_account(&ctx.merchant_did_pda).unwrap();
    assert_eq!(acct.data().len(), 153, "DID account should be 153 bytes");
    assert_eq!(
        &acct.data()[OFF_ORIGINAL_PK..OFF_ORIGINAL_PK + 32],
        ctx.merchant_keypair.pubkey().as_ref(),
        "original_pk should match merchant"
    );
    assert_eq!(
        &acct.data()[OFF_CONTROLLER_PK..OFF_CONTROLLER_PK + 32],
        ctx.merchant_keypair.pubkey().as_ref(),
        "controller_pk should match merchant"
    );
    assert_eq!(&acct.data()[OFF_VC_HASH..OFF_VC_HASH + 32], &vc_hash, "vc_hash should match");
    assert_eq!(ctx.did_nonce(), 0, "initial nonce should be 0");
    println!("PASS: initialize_did");
}

#[test]
fn test_update_did_with_vc() {
    let mut ctx = Ctx::new();
    let vc_hash_1 = [0xAA; 32];
    let sig1 = ctx.make_platform_signature(&vc_hash_1, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash_1, sig1).unwrap();

    let vc_hash_2 = [0xBB; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 0, sig2, &merchant);
    assert!(result.is_ok(), "update_did_with_vc should succeed: {:?}", result);

    assert_eq!(ctx.did_vc_hash(), vc_hash_2, "vc_hash should be updated");
    assert_eq!(ctx.did_nonce(), 1, "nonce should increment to 1");
    println!("PASS: update_did_with_vc");
}

#[test]
fn test_set_recovery_key() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    let result = ctx.set_recovery_key(recovery_pk, 0, &merchant);
    assert!(result.is_ok(), "set_recovery_key should succeed: {:?}", result);

    assert_eq!(ctx.did_recovery_pk(), ctx.recovery_keypair.pubkey(), "recovery_pk should match");
    assert_eq!(ctx.did_nonce(), 1, "nonce should increment to 1");
    println!("PASS: set_recovery_key");
}

#[test]
fn test_recover_controller() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 0, &merchant).unwrap();

    let new_controller = kp(4);
    let recovery = kp(3);
    let result = ctx.recover_controller(new_controller.pubkey(), 1, &recovery);
    assert!(result.is_ok(), "recover_controller should succeed: {:?}", result);

    assert_eq!(ctx.did_controller_pk(), new_controller.pubkey(), "controller should be updated");
    assert_eq!(ctx.did_nonce(), 2, "nonce should increment to 2");
    println!("PASS: recover_controller");
}

#[test]
fn test_revoke_vc() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash], &ctx.pid);

    let merchant_pk = ctx.merchant_keypair.pubkey();
    let platform = kp(1);
    let result = ctx.revoke_vc(vc_hash, merchant_pk, 1, &platform, revoked_vc_pda);
    assert!(result.is_ok(), "revoke_vc should succeed: {:?}", result);

    let acct = ctx.svm.get_account(&revoked_vc_pda).expect("revoked_vc PDA should exist");
    assert_eq!(acct.data().len(), 114, "RevokedVc should be 114 bytes");
    println!("PASS: revoke_vc");
}

#[test]
fn test_full_lifecycle() {
    let mut ctx = Ctx::new();

    // 2. initialize_did
    let vc_hash_1 = [0x11; 32];
    let sig1 = ctx.make_platform_signature(&vc_hash_1, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash_1, sig1).unwrap();
    assert_eq!(ctx.did_nonce(), 0);
    assert_eq!(ctx.did_vc_hash(), vc_hash_1);

    // 3. update_did_with_vc (nonce 0 -> 1)
    let vc_hash_2 = [0x22; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    ctx.update_did_with_vc(vc_hash_2, 0, sig2, &merchant).unwrap();
    assert_eq!(ctx.did_nonce(), 1);
    assert_eq!(ctx.did_vc_hash(), vc_hash_2);

    // 4. set_recovery_key (nonce 1 -> 2)
    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 1, &merchant).unwrap();
    assert_eq!(ctx.did_nonce(), 2);
    assert_eq!(ctx.did_recovery_pk(), ctx.recovery_keypair.pubkey());

    // 5. recover_controller (nonce 2 -> 3)
    let new_controller = kp(4);
    let recovery = kp(3);
    ctx.recover_controller(new_controller.pubkey(), 2, &recovery).unwrap();
    assert_eq!(ctx.did_nonce(), 3);
    assert_eq!(ctx.did_controller_pk(), new_controller.pubkey());

    // 6. update_did_with_vc as new controller (nonce 3 -> 4)
    let vc_hash_3 = [0x33; 32];
    let sig3 = ctx.make_platform_signature(&vc_hash_3, &new_controller.pubkey());
    fund(&mut ctx.svm, &new_controller.pubkey(), 100_000_000_000);
    ctx.update_did_with_vc(vc_hash_3, 3, sig3, &new_controller).unwrap();
    assert_eq!(ctx.did_nonce(), 4);
    assert_eq!(ctx.did_vc_hash(), vc_hash_3);

    // 7. revoke_vc
    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash_3], &ctx.pid);
    let merchant_pk = ctx.merchant_keypair.pubkey();
    let platform = kp(1);
    ctx.revoke_vc(vc_hash_3, merchant_pk, 2, &platform, revoked_vc_pda).unwrap();
    assert!(ctx.svm.get_account(&revoked_vc_pda).is_some());

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
    assert!(result.is_err(), "Should fail with wrong platform signature");
    println!("PASS: initialize_did_wrong_signature");
}

#[test]
fn test_update_did_wrong_nonce() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let vc_hash_2 = [0xBB; 32];
    let merchant = kp(2);
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &merchant.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 1, sig2, &merchant);
    assert!(result.is_err(), "Should fail with wrong nonce");
    println!("PASS: update_did_wrong_nonce");
}

#[test]
fn test_update_did_wrong_controller() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let impostor = kp(99);
    fund(&mut ctx.svm, &impostor.pubkey(), 100_000_000_000);

    let vc_hash_2 = [0xBB; 32];
    let sig2 = ctx.make_platform_signature(&vc_hash_2, &impostor.pubkey());
    let result = ctx.update_did_with_vc(vc_hash_2, 0, sig2, &impostor);
    assert!(result.is_err(), "Should fail: signer is not controller");
    println!("PASS: update_did_wrong_controller");
}

#[test]
fn test_recover_controller_wrong_key() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let recovery_pk = ctx.recovery_keypair.pubkey();
    let merchant = kp(2);
    ctx.set_recovery_key(recovery_pk, 0, &merchant).unwrap();

    let new_controller = kp(4);
    let merchant = kp(2);
    let result = ctx.recover_controller(new_controller.pubkey(), 1, &merchant);
    assert!(result.is_err(), "Should fail: wrong recovery key");
    println!("PASS: recover_controller_wrong_key");
}

#[test]
fn test_revoke_vc_unauthorized() {
    let mut ctx = Ctx::new();
    let vc_hash = [0xAA; 32];
    let sig = ctx.make_platform_signature(&vc_hash, &ctx.merchant_keypair.pubkey());
    ctx.initialize_did(vc_hash, sig).unwrap();

    let (revoked_vc_pda, _) =
        Pubkey::find_program_address(&[b"revoked-vc", &vc_hash], &ctx.pid);

    let impostor = kp(99);
    fund(&mut ctx.svm, &impostor.pubkey(), 100_000_000_000);

    let merchant_pk = ctx.merchant_keypair.pubkey();
    let result = ctx.revoke_vc(vc_hash, merchant_pk, 1, &impostor, revoked_vc_pda);
    assert!(result.is_err(), "Should fail: unauthorized revocation");
    println!("PASS: revoke_vc_unauthorized");
}
