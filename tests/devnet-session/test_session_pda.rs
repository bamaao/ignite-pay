//! Devnet integration test for session program PDA custody model.
//!
//! Tests:
//! 1. Register session key → PDA created
//! 2. Fund PDA with SOL + fund ephemeral key with gas SOL
//! 3. execute_payment → SOL from PDA via invoke_signed
//! 4. withdraw_remaining → SOL back to owner
//! 5. Register another session for SPL test
//!
//! Run: cargo test --test test_session_pda -- --nocapture

use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    system_program,
    sysvar,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};
use spl_token;
use std::str::FromStr;

const PROGRAM_ID: &str = "Avu35SYnvcSpWeYQhC7w2XT6DCurhnYB5PdajTqet9o";
// Devnet USDC mint
const USDC_MINT: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";

fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let hash = solana_sdk::hash::hash(preimage.as_bytes());
    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&hash.to_bytes()[..8]);
    sighash
}

fn derive_session_pda(owner: &Pubkey, ephemeral: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"session", owner.as_ref(), ephemeral.as_ref()],
        program_id,
    )
}

fn build_register_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
    ephemeral: &Pubkey,
    target_program: &Pubkey,
    expires_at: i64,
    spending_limit: u64,
    scopes: Vec<String>,
    token_mint: &Pubkey,
    per_tx_limit: u64,
    daily_tx_count_limit: u32,
) -> Instruction {
    let sighash = anchor_sighash("register_session_key");
    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(target_program.as_ref());
    data.extend_from_slice(&expires_at.to_le_bytes());
    data.extend_from_slice(&spending_limit.to_le_bytes());
    let scopes_len = scopes.len() as u32;
    data.extend_from_slice(&scopes_len.to_le_bytes());
    for scope in &scopes {
        let scope_bytes = scope.as_bytes();
        let scope_len = scope_bytes.len() as u32;
        data.extend_from_slice(&scope_len.to_le_bytes());
        data.extend_from_slice(scope_bytes);
    }
    data.extend_from_slice(token_mint.as_ref());
    data.extend_from_slice(&per_tx_limit.to_le_bytes());
    data.extend_from_slice(&daily_tx_count_limit.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(*ephemeral, true),
            AccountMeta::new_readonly(*target_program, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
        ],
        data,
    }
}

fn build_execute_payment_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    ephemeral: &Pubkey,
    recipient: &Pubkey,
    amount: u64,
    scope: &str,
) -> Instruction {
    let sighash = anchor_sighash("execute_payment");
    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(&amount.to_le_bytes());
    let scope_bytes = scope.as_bytes();
    let scope_len = scope_bytes.len() as u32;
    data.extend_from_slice(&scope_len.to_le_bytes());
    data.extend_from_slice(scope_bytes);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new_readonly(*ephemeral, true),
            AccountMeta::new(*recipient, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
        ],
        data,
    }
}

fn build_withdraw_remaining_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
    recipient: &Pubkey,
) -> Instruction {
    let sighash = anchor_sighash("withdraw_remaining");
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*recipient, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: sighash.to_vec(),
    }
}

fn build_execute_spl_payment_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    ephemeral: &Pubkey,
    source_ata: &Pubkey,
    dest_ata: &Pubkey,
    token_mint: &Pubkey,
    amount: u64,
    scope: &str,
) -> Instruction {
    let sighash = anchor_sighash("execute_spl_payment");
    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(&amount.to_le_bytes());
    let scope_bytes = scope.as_bytes();
    let scope_len = scope_bytes.len() as u32;
    data.extend_from_slice(&scope_len.to_le_bytes());
    data.extend_from_slice(scope_bytes);

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new_readonly(*ephemeral, true),
            AccountMeta::new(*source_ata, false),
            AccountMeta::new(*dest_ata, false),
            AccountMeta::new_readonly(*token_mint, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
        ],
        data,
    }
}

fn build_withdraw_spl_remaining_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
    source_ata: &Pubkey,
    dest_ata: &Pubkey,
    amount: u64,
) -> Instruction {
    let sighash = anchor_sighash("withdraw_spl_remaining");
    let mut data = Vec::new();
    data.extend_from_slice(&sighash);
    data.extend_from_slice(&amount.to_le_bytes());

    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*source_ata, false),
            AccountMeta::new(*dest_ata, false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data,
    }
}

fn build_close_session_ix(
    program_id: &Pubkey,
    session_pda: &Pubkey,
    owner: &Pubkey,
) -> Instruction {
    let sighash = anchor_sighash("close_session");
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*session_pda, false),
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
        ],
        data: sighash.to_vec(),
    }
}

fn get_rpc_client() -> solana_client::rpc_client::RpcClient {
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/rp48bxJupDZFzZaepdZyc";
    solana_client::rpc_client::RpcClient::new_with_commitment(
        rpc_url.to_string(),
        CommitmentConfig::confirmed(),
    )
}

fn load_payer() -> Keypair {
    let keypair_path = dirs::home_dir()
        .unwrap()
        .join(".config/solana/id.json");
    let keypair_str = std::fs::read_to_string(keypair_path).expect("Failed to read keypair file");
    let keypair_bytes: Vec<u8> = serde_json::from_str(&keypair_str).expect("Failed to parse keypair");
    Keypair::from_bytes(&keypair_bytes).expect("Failed to create keypair")
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_pda_custody_flow() {
    let client = get_rpc_client();
    let payer = load_payer();
    let program_id = Pubkey::from_str(PROGRAM_ID).unwrap();
    let owner = payer.pubkey();

    println!("=== PDA Custody Integration Test ===");
    println!("Owner: {}", owner);

    // Generate ephemeral keypair
    let ephemeral_keypair = Keypair::new();
    let ephemeral_pubkey = ephemeral_keypair.pubkey();
    println!("Ephemeral: {}", ephemeral_pubkey);

    // Derive session PDA
    let (session_pda, bump) = derive_session_pda(&owner, &ephemeral_pubkey, &program_id);
    println!("Session PDA: {} (bump: {})", session_pda, bump);

    let recent_blockhash = client.get_latest_blockhash().unwrap();

    // ─── Step 1: Register session key ───
    println!("\n--- Step 1: Register session key ---");
    let target_program = system_program::id();
    // Get current slot time and set expires_at to 1 hour from now
    let slot_info = client.get_slot().unwrap();
    let block_time = client.get_block_time(slot_info).unwrap_or(0);
    let expires_at = block_time + 3600; // 1 hour from now
    let spending_limit = 1_000_000_000u64; // 1 SOL
    let scopes = vec!["sol:transfer".to_string()];
    let token_mint = Pubkey::default(); // SOL session
    let per_tx_limit = 500_000_000u64;
    let daily_tx_count_limit = 10u32;

    let register_ix = build_register_ix(
        &program_id,
        &session_pda,
        &owner,
        &ephemeral_pubkey,
        &target_program,
        expires_at,
        spending_limit,
        scopes,
        &token_mint,
        per_tx_limit,
        daily_tx_count_limit,
    );

    let tx = Transaction::new_signed_with_payer(
        &[register_ix],
        Some(&owner),
        &[&payer, &ephemeral_keypair],
        recent_blockhash,
    );

    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Register tx: {}", sig);

    // Verify PDA exists
    let pda_info = client.get_account(&session_pda).unwrap();
    println!("PDA account created, lamports: {}", pda_info.lamports);

    // ─── Step 2: Fund PDA with SOL + gas for ephemeral ───
    println!("\n--- Step 2: Fund PDA and ephemeral key ---");
    let fund_pda_amount = 500_000_000u64; // 0.5 SOL to PDA
    let gas_amount = 10_000_000u64; // 0.01 SOL gas for ephemeral

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let fund_ix1 = system_instruction::transfer(&owner, &session_pda, fund_pda_amount);
    let fund_ix2 = system_instruction::transfer(&owner, &ephemeral_pubkey, gas_amount);

    let tx = Transaction::new_signed_with_payer(
        &[fund_ix1, fund_ix2],
        Some(&owner),
        &[&payer],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Fund tx: {}", sig);

    let pda_balance = client.get_balance(&session_pda).unwrap();
    let ephemeral_balance = client.get_balance(&ephemeral_pubkey).unwrap();
    println!("PDA balance: {} lamports", pda_balance);
    println!("Ephemeral balance: {} lamports", ephemeral_balance);
    assert!(pda_balance >= fund_pda_amount, "PDA should have SOL");
    assert!(ephemeral_balance >= gas_amount, "Ephemeral should have gas SOL");

    // ─── Step 3: Execute SOL payment from PDA ───
    println!("\n--- Step 3: Execute SOL payment from PDA ---");
    let recipient = Keypair::new().pubkey();
    let payment_amount = 100_000_000u64; // 0.1 SOL

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let execute_ix = build_execute_payment_ix(
        &program_id,
        &session_pda,
        &ephemeral_pubkey,
        &recipient,
        payment_amount,
        "sol:transfer",
    );

    let tx = Transaction::new_signed_with_payer(
        &[execute_ix],
        Some(&ephemeral_pubkey), // ephemeral pays gas
        &[&ephemeral_keypair],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Execute payment tx: {}", sig);

    let recipient_balance = client.get_balance(&recipient).unwrap();
    let pda_balance_after = client.get_balance(&session_pda).unwrap();
    println!("Recipient received: {} lamports", recipient_balance);
    println!("PDA balance after: {} lamports", pda_balance_after);
    assert_eq!(recipient_balance, payment_amount, "Recipient should receive exact amount");
    assert!(pda_balance_after < pda_balance, "PDA balance should decrease");

    // ─── Step 4: Second payment to verify spending limit tracking ───
    println!("\n--- Step 4: Second payment ---");
    let recipient2 = Keypair::new().pubkey();
    let payment_amount2 = 50_000_000u64; // 0.05 SOL

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let execute_ix2 = build_execute_payment_ix(
        &program_id,
        &session_pda,
        &ephemeral_pubkey,
        &recipient2,
        payment_amount2,
        "sol:transfer",
    );

    let tx = Transaction::new_signed_with_payer(
        &[execute_ix2],
        Some(&ephemeral_pubkey),
        &[&ephemeral_keypair],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Second payment tx: {}", sig);
    println!("Recipient2 received: {} lamports", client.get_balance(&recipient2).unwrap());

    // ─── Step 5: Withdraw remaining SOL from PDA ───
    println!("\n--- Step 5: Withdraw remaining SOL from PDA ---");
    let pda_before_withdraw = client.get_balance(&session_pda).unwrap();
    println!("PDA balance before withdraw: {} lamports", pda_before_withdraw);

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let withdraw_ix = build_withdraw_remaining_ix(&program_id, &session_pda, &owner, &owner);

    let tx = Transaction::new_signed_with_payer(
        &[withdraw_ix],
        Some(&owner), // owner is fee payer for withdraw
        &[&payer],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Withdraw tx: {}", sig);

    let pda_after_withdraw = client.get_balance(&session_pda).unwrap();
    println!("PDA balance after withdraw: {} lamports", pda_after_withdraw);
    // PDA should still have rent-exempt minimum
    assert!(pda_after_withdraw < pda_before_withdraw, "PDA balance should decrease after withdraw");

    // ─── Step 6: Close session ───
    println!("\n--- Step 6: Revoke and close session ---");
    let revoke_sighash = anchor_sighash("revoke_session");
    let revoke_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(session_pda, false),
            AccountMeta::new_readonly(owner, true),
        ],
        data: revoke_sighash.to_vec(),
    };

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[revoke_ix],
        Some(&owner),
        &[&payer],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Revoke tx: {}", sig);

    let close_ix = build_close_session_ix(&program_id, &session_pda, &owner);
    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[close_ix],
        Some(&owner),
        &[&payer],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Close tx: {}", sig);

    let pda_after_close = client.get_account(&session_pda);
    assert!(pda_after_close.is_err(), "PDA should be closed (account not found)");
    println!("PDA closed successfully (account no longer exists)");

    println!("\n=== All SOL tests passed! ===");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spl_payment_pda_custody() {
    let client = get_rpc_client();
    let payer = load_payer();
    let program_id = Pubkey::from_str(PROGRAM_ID).unwrap();
    let owner = payer.pubkey();
    let usdc_mint = Pubkey::from_str(USDC_MINT).unwrap();

    println!("\n=== SPL Payment PDA Custody Test ===");
    println!("Owner: {}", owner);

    let ephemeral_keypair = Keypair::new();
    let ephemeral_pubkey = ephemeral_keypair.pubkey();
    let (session_pda, bump) = derive_session_pda(&owner, &ephemeral_pubkey, &program_id);
    println!("Ephemeral: {}", ephemeral_pubkey);
    println!("Session PDA: {} (bump: {})", session_pda, bump);

    // Derive ATAs
    let pda_ata = get_associated_token_address_with_program_id(&session_pda, &usdc_mint, &spl_token::id());
    let owner_ata = get_associated_token_address_with_program_id(&owner, &usdc_mint, &spl_token::id());

    // ─── Step 1: Register SPL session ───
    println!("\n--- Step 1: Register SPL session ---");
    let target_program = spl_token::id();
    let slot_info = client.get_slot().unwrap();
    let block_time = client.get_block_time(slot_info).unwrap_or(0);
    let expires_at = block_time + 3600; // 1 hour from now
    let spending_limit = 10_000_000u64; // 10 USDC
    let scopes = vec!["sol:transfer".to_string(), "spl:transfer".to_string()];
    let per_tx_limit = 5_000_000u64;
    let daily_tx_count_limit = 50u32;

    let register_ix = build_register_ix(
        &program_id,
        &session_pda,
        &owner,
        &ephemeral_pubkey,
        &target_program,
        expires_at,
        spending_limit,
        scopes,
        &usdc_mint,
        per_tx_limit,
        daily_tx_count_limit,
    );

    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[register_ix],
        Some(&owner),
        &[&payer, &ephemeral_keypair],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Register tx: {}", sig);

    // ─── Step 2: Create PDA ATA + fund gas SOL ───
    println!("\n--- Step 2: Create PDA ATA + fund gas ---");
    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let create_ata_ix = create_associated_token_account(
        &owner,
        &session_pda,
        &usdc_mint,
        &spl_token::id(),
    );
    let gas_ix = system_instruction::transfer(&owner, &ephemeral_pubkey, 10_000_000);

    let tx = Transaction::new_signed_with_payer(
        &[create_ata_ix, gas_ix],
        Some(&owner),
        &[&payer],
        recent_blockhash,
    );
    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Create ATA + gas tx: {}", sig);

    // ─── Step 3: Transfer USDC from owner ATA to PDA ATA ───
    println!("\n--- Step 3: Fund PDA ATA with USDC ---");
    // Check owner's USDC balance first
    let owner_usdc_balance = client.get_token_account_balance(&owner_ata);
    match owner_usdc_balance {
        Ok(resp) => {
            let amount: u64 = resp.amount.parse().unwrap();
            println!("Owner USDC balance: {} ({})", &resp.ui_amount_string, amount);
            if amount == 0 {
                println!("Owner has no USDC, skipping SPL test. Run: solana airdrop --mint 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU <amount>");
                return;
            }

            let transfer_amount = amount.min(1_000_000); // max 1 USDC
            let recent_blockhash = client.get_latest_blockhash().unwrap();

            // Build SPL token transfer from owner_ata to pda_ata
            let transfer_ix = spl_token::instruction::transfer(
                &spl_token::id(),
                &owner_ata,
                &pda_ata,
                &owner,
                &[&owner],
                transfer_amount,
            ).unwrap();

            let tx = Transaction::new_signed_with_payer(
                &[transfer_ix],
                Some(&owner),
                &[&payer],
                recent_blockhash,
            );
            let sig = client.send_and_confirm_transaction(&tx).unwrap();
            println!("Fund USDC tx: {}", sig);

            // ─── Step 4: Execute SPL payment from PDA ATA ───
            println!("\n--- Step 4: Execute SPL payment from PDA ATA ---");
            let merchant = Keypair::new().pubkey();
            let merchant_ata = get_associated_token_address_with_program_id(&merchant, &usdc_mint, &spl_token::id());

            // Create merchant ATA (owner pays)
            let create_merchant_ata = create_associated_token_account(
                &owner,
                &merchant,
                &usdc_mint,
                &spl_token::id(),
            );
            let recent_blockhash = client.get_latest_blockhash().unwrap();
            let tx = Transaction::new_signed_with_payer(
                &[create_merchant_ata],
                Some(&owner),
                &[&payer],
                recent_blockhash,
            );
            let sig = client.send_and_confirm_transaction(&tx).unwrap();
            println!("Create merchant ATA tx: {}", sig);

            let spl_payment_amount = 100_000u64; // 0.1 USDC
            let execute_spl_ix = build_execute_spl_payment_ix(
                &program_id,
                &session_pda,
                &ephemeral_pubkey,
                &pda_ata,
                &merchant_ata,
                &usdc_mint,
                spl_payment_amount,
                "spl:transfer",
            );

            let recent_blockhash = client.get_latest_blockhash().unwrap();
            let tx = Transaction::new_signed_with_payer(
                &[execute_spl_ix],
                Some(&ephemeral_pubkey),
                &[&ephemeral_keypair],
                recent_blockhash,
            );
            let sig = client.send_and_confirm_transaction(&tx).unwrap();
            println!("Execute SPL payment tx: {}", sig);

            // Verify merchant received USDC
            let merchant_balance = client.get_token_account_balance(&merchant_ata).unwrap();
            println!("Merchant received: {} USDC", &merchant_balance.ui_amount_string);
            assert_eq!(merchant_balance.amount.parse::<u64>().unwrap(), spl_payment_amount);

            // ─── Step 5: Withdraw remaining SPL from PDA ATA ───
            println!("\n--- Step 5: Withdraw remaining SPL from PDA ATA ---");
            let pda_usdc = client.get_token_account_balance(&pda_ata).unwrap();
            let remaining: u64 = pda_usdc.amount.parse().unwrap();
            println!("PDA ATA remaining USDC: {}", remaining);

            if remaining > 0 {
                let withdraw_spl_ix = build_withdraw_spl_remaining_ix(
                    &program_id,
                    &session_pda,
                    &owner,
                    &pda_ata,
                    &owner_ata,
                    remaining,
                );

                let recent_blockhash = client.get_latest_blockhash().unwrap();
                let tx = Transaction::new_signed_with_payer(
                    &[withdraw_spl_ix],
                    Some(&owner),
                    &[&payer],
                    recent_blockhash,
                );
                let sig = client.send_and_confirm_transaction(&tx).unwrap();
                println!("Withdraw SPL tx: {}", sig);
                println!("SPL tokens withdrawn to owner ATA");
            }

            // ─── Step 6: Withdraw SOL from PDA + close ───
            println!("\n--- Step 6: Withdraw SOL and close ---");
            let withdraw_ix = build_withdraw_remaining_ix(&program_id, &session_pda, &owner, &owner);
            let revoke_ix = Instruction {
                program_id,
                accounts: vec![
                    AccountMeta::new(session_pda, false),
                    AccountMeta::new_readonly(owner, true),
                ],
                data: anchor_sighash("revoke_session").to_vec(),
            };
            let close_ix = build_close_session_ix(&program_id, &session_pda, &owner);

            let recent_blockhash = client.get_latest_blockhash().unwrap();
            let tx = Transaction::new_signed_with_payer(
                &[withdraw_ix, revoke_ix, close_ix],
                Some(&owner),
                &[&payer],
                recent_blockhash,
            );
            let sig = client.send_and_confirm_transaction(&tx).unwrap();
            println!("Withdraw + revoke + close tx: {}", sig);

            println!("\n=== All SPL tests passed! ===");
        }
        Err(e) => {
            println!("Owner has no USDC ATA or no USDC: {}", e);
            println!("Skipping SPL test. To fund: solana airdrop --mint 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU 1");
        }
    }
}
