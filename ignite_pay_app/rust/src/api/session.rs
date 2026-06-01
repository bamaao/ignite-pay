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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;
use std::time::{SystemTime, UNIX_EPOCH};

/// Request a devnet SOL airdrop (2 SOL) to the given pubkey.
async fn devnet_airdrop(rpc_url: &str, pubkey_b58: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "requestAirdrop",
            "params": [pubkey_b58, 2_000_000_000]
        }))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    if let Some(err) = body.get("error") {
        return Err(anyhow::anyhow!("Airdrop failed: {}", err));
    }
    let signature = body["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No airdrop signature in response"))?;
    // Wait for confirmation
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    Ok(signature.to_string())
}

/// Session key information exposed to Flutter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKeyInfo {
    /// Base58-encoded ephemeral public key.
    pub ephemeral_pubkey: String,
    /// Base58-encoded ephemeral secret key (64-byte keypair).
    pub ephemeral_secret_key: String,
    /// Unix timestamp when the session expires.
    pub expires_at: i64,
    /// Maximum spending limit in lamports.
    pub spending_limit: u64,
    /// Permission scopes (e.g. ["sol:transfer", "spl:transfer"]).
    pub scopes: Vec<String>,
    /// On-chain registration transaction signature.
    pub tx_signature: Option<String>,
    /// On-chain session PDA address.
    pub session_pda: Option<String>,
}

/// A session key entry returned for listing / query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKeyEntry {
    /// Base58-encoded ephemeral public key.
    pub ephemeral_pubkey: String,
    /// Unix timestamp when the session expires.
    pub expires_at: i64,
    /// Maximum spending limit in lamports.
    pub spending_limit: u64,
    /// On-chain registration transaction signature (if registered on-chain).
    pub tx_signature: Option<String>,
    /// On-chain session PDA address (if registered on-chain).
    pub session_pda: Option<String>,
    /// Status: "active", "expired", or "unknown".
    pub status: String,
    /// Per-transaction spending limit in lamports (0 = no limit).
    pub per_tx_limit: u64,
    /// Daily transaction count limit (0 = no limit).
    pub daily_tx_count_limit: u32,
}

/// An unsigned register transaction ready for external wallet signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedRegisterTx {
    /// Base58-encoded unsigned transaction bytes.
    pub unsigned_tx_b58: String,
    /// Derived session PDA address (base58).
    pub session_pda: String,
    /// Base58-encoded ephemeral public key.
    pub ephemeral_pubkey: String,
}

/// Create a session key for payment authorization.
///
/// Deprecated: session keys are created by MCP; the phone only registers the MCP-provided
/// ephemeral pubkey on-chain via an external wallet.
pub fn create_session_key(
    storage_path: String,
    _owner_pubkey: String,
    _target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    per_tx_limit: u64,
    daily_tx_count_limit: u32,
) -> Result<SessionKeyInfo> {
    let _ = (
        storage_path,
        scopes,
        spending_limit,
        duration_secs,
        per_tx_limit,
        daily_tx_count_limit,
    );
    Err(anyhow::anyhow!(
        "Phone must not generate session keys; use MCP PaymentRequest + wallet registration"
    ))
}

/// Create a session key and register it on-chain via JSON-RPC.
///
/// Since solana-sdk can't compile on Windows (OpenSSL dependency),
/// this uses raw JSON-RPC via reqwest + ed25519-dalek to:
/// 1. Generate ephemeral keypair
/// 2. Derive session PDA
/// 3. Build Anchor instruction data
/// 4. Build and sign raw transaction
/// 5. Submit via JSON-RPC sendTransaction
pub async fn create_and_register_session_key(
    storage_path: String,
    rpc_url: String,
    owner_secret_key: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
) -> Result<SessionKeyInfo> {
    let _ = (
        storage_path,
        rpc_url,
        owner_secret_key,
        target_program,
        scopes,
        spending_limit,
        duration_secs,
    );
    Err(anyhow::anyhow!(
        "Phone must not register session keys locally; use wallet + MCP PaymentRequest flow"
    ))
}

/// Register an externally-provided session key on-chain.
/// Used when MCP creates the ephemeral keypair and the phone just needs to register it.
/// The logic mirrors `create_and_register_session_key()` but skips keypair generation.
pub async fn register_external_session_key(
    storage_path: String,
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    ephemeral_secret_key: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    token_mint: Option<String>,
) -> Result<SessionKeyInfo> {
    let _ = (
        storage_path,
        rpc_url,
        owner_secret_key,
        ephemeral_pubkey,
        ephemeral_secret_key,
        target_program,
        scopes,
        spending_limit,
        duration_secs,
        token_mint,
    );
    Err(anyhow::anyhow!(
        "Phone must not register session keys with ephemeral secret; use wallet + MCP pubkey flow"
    ))
}

/// Fund a session key by transferring SOL (and optionally SPL token) from the owner.
/// Sends SOL to the session PDA and SPL tokens to the PDA's ATA.
/// Also sends a small amount of SOL (0.01 SOL) to the ephemeral key for gas fees.
/// Uses raw JSON-RPC to build System Program transfer (and Token Program transfer) instructions.
pub async fn fund_session_key(
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    sol_amount: u64,
    spl_token_mint: Option<String>,
    spl_amount: Option<u64>,
) -> Result<Vec<String>> {
    let mut signatures = Vec::new();
    let client = reqwest::Client::new();

    // Decode owner keypair
    let owner_keypair_bytes = bs58::decode(&owner_secret_key).into_vec()?;
    if owner_keypair_bytes.len() != 64 {
        return Err(anyhow::anyhow!("Invalid owner keypair length"));
    }
    let owner_signing =
        ed25519_dalek::SigningKey::from_bytes(&owner_keypair_bytes[..32].try_into().unwrap());
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();

    // Decode ephemeral pubkey
    let ephemeral_bytes = bs58::decode(&ephemeral_pubkey).into_vec()?;
    if ephemeral_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid ephemeral pubkey"));
    }
    let ephemeral_pubkey_arr: [u8; 32] = ephemeral_bytes.try_into().unwrap();

    // Derive session PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_arr);

    // System program
    let system_program = get_system_program_id_bytes();

    // --- SOL transfer to PDA ---
    if sol_amount > 0 {
        let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
        let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
        let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

        let account_keys: Vec<[u8; 32]> = vec![
            owner_pubkey_bytes,
            session_pda,
            system_program,
        ];

        let mut message = Vec::new();
        message.push(1); // num_required_signatures
        message.push(0); // num_readonly_signed
        message.push(1); // num_readonly_unsigned (system_program)
        compact_u64_encode(&mut message, account_keys.len() as u64);
        for key in &account_keys {
            message.extend_from_slice(key);
        }
        message.extend_from_slice(&blockhash_arr);
        compact_u64_encode(&mut message, 1);
        message.push(2); // program_id_index = system_program
        let ix_accounts: Vec<u8> = vec![0, 1]; // [owner, session_pda]
        compact_u64_encode(&mut message, ix_accounts.len() as u64);
        message.extend_from_slice(&ix_accounts);
        let mut ix_data = Vec::with_capacity(12);
        ix_data.extend_from_slice(&2u32.to_le_bytes()); // SystemInstruction::Transfer
        ix_data.extend_from_slice(&sol_amount.to_le_bytes());
        compact_u64_encode(&mut message, ix_data.len() as u64);
        message.extend_from_slice(&ix_data);

        // Sign
        use ed25519_dalek::Signer;
        let msg_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&message);
            let hash = hasher.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash);
            arr
        };
        let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

        let mut tx = Vec::new();
        compact_u64_encode(&mut tx, 1);
        tx.extend_from_slice(&owner_sig);
        tx.extend_from_slice(&message);

        let sig = send_transaction(&client, &rpc_url, &tx).await?;
        signatures.push(sig);
    }

    // --- Gas SOL to ephemeral key (0.01 SOL = 10_000_000 lamports) ---
    {
        let gas_amount: u64 = 10_000_000;
        let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
        let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
        let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

        let account_keys: Vec<[u8; 32]> = vec![
            owner_pubkey_bytes,
            ephemeral_pubkey_arr,
            system_program,
        ];

        let mut message = Vec::new();
        message.push(1);
        message.push(0);
        message.push(1);
        compact_u64_encode(&mut message, account_keys.len() as u64);
        for key in &account_keys {
            message.extend_from_slice(key);
        }
        message.extend_from_slice(&blockhash_arr);
        compact_u64_encode(&mut message, 1);
        message.push(2);
        let ix_accounts: Vec<u8> = vec![0, 1];
        compact_u64_encode(&mut message, ix_accounts.len() as u64);
        message.extend_from_slice(&ix_accounts);
        let mut ix_data = Vec::with_capacity(12);
        ix_data.extend_from_slice(&2u32.to_le_bytes()); // SystemInstruction::Transfer
        ix_data.extend_from_slice(&gas_amount.to_le_bytes());
        compact_u64_encode(&mut message, ix_data.len() as u64);
        message.extend_from_slice(&ix_data);

        use ed25519_dalek::Signer;
        let msg_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&message);
            let hash = hasher.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash);
            arr
        };
        let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

        let mut tx = Vec::new();
        compact_u64_encode(&mut tx, 1);
        tx.extend_from_slice(&owner_sig);
        tx.extend_from_slice(&message);

        let sig = send_transaction(&client, &rpc_url, &tx).await?;
        signatures.push(sig);
    }

    // --- SPL token transfer to PDA's ATA (optional) ---
    if let (Some(mint_b58), Some(amount)) = (spl_token_mint, spl_amount) {
        if amount > 0 {
            let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .into_vec()
                .unwrap()
                .try_into()
                .unwrap();

            let mint_bytes: [u8; 32] = bs58::decode(&mint_b58)
                .into_vec()?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid token mint"))?;

            // Derive ATAs: source = owner's ATA, destination = PDA's ATA
            let owner_ata = derive_ata(&owner_pubkey_bytes, &mint_bytes);
            let pda_ata = derive_ata(&session_pda, &mint_bytes);

            let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
            let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
            let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

            let account_keys: Vec<[u8; 32]> = vec![
                owner_pubkey_bytes,
                owner_ata,
                pda_ata,
                token_program,
            ];

            let mut message = Vec::new();
            message.push(1); // num_required_signatures
            message.push(0); // num_readonly_signed
            message.push(1); // num_readonly_unsigned (token_program)
            compact_u64_encode(&mut message, account_keys.len() as u64);
            for key in &account_keys {
                message.extend_from_slice(key);
            }
            message.extend_from_slice(&blockhash_arr);
            compact_u64_encode(&mut message, 1);
            message.push(3); // program_id_index = token_program
            let ix_accounts: Vec<u8> = vec![1, 2, 0]; // [source_ata, dest_ata, authority]
            compact_u64_encode(&mut message, ix_accounts.len() as u64);
            message.extend_from_slice(&ix_accounts);
            // SPL Token program instruction layout:
            //   Transfer = 1-byte discriminator (3) + 8-byte amount LE
            let mut ix_data = Vec::with_capacity(9);
            ix_data.push(3); // Transfer
            ix_data.extend_from_slice(&amount.to_le_bytes());
            compact_u64_encode(&mut message, ix_data.len() as u64);
            message.extend_from_slice(&ix_data);

            use ed25519_dalek::Signer;
            let msg_hash = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&message);
                let hash = hasher.finalize();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&hash);
                arr
            };
            let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

            let mut tx = Vec::new();
            compact_u64_encode(&mut tx, 1);
            tx.extend_from_slice(&owner_sig);
            tx.extend_from_slice(&message);

            let sig = send_transaction(&client, &rpc_url, &tx).await?;
            signatures.push(sig);
        }
    }

    Ok(signatures)
}

/// Register an externally-provided session key and fund it in one operation.
/// Calls `register_external_session_key` then `fund_session_key`.
pub async fn register_and_fund_session_key(
    storage_path: String,
    rpc_url: String,
    owner_secret_key: String,
    ephemeral_pubkey: String,
    ephemeral_secret_key: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    token_mint: Option<String>,
    sol_funding: u64,
    token_funding: Option<u64>,
) -> Result<SessionKeyInfo> {
    // Resolve owner key: derive from DID if empty
    let resolved_owner_key = if owner_secret_key.is_empty() {
        let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
        let did = identity_mgr.did();
        let owner_seed = sha2::Sha256::digest(did.as_bytes());
        let owner_seed_bytes: &[u8; 32] = owner_seed
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
        let signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
        let pubkey = signing.verifying_key().to_bytes();
        let mut kp_bytes = signing.to_bytes().to_vec();
        kp_bytes.extend_from_slice(&pubkey);
        bs58::encode(&kp_bytes).into_string()
    } else {
        owner_secret_key
    };

    // 0. Extract owner pubkey for airdrop check
    let owner_kp_bytes = bs58::decode(&resolved_owner_key).into_vec()?;
    if owner_kp_bytes.len() != 64 {
        return Err(anyhow::anyhow!("Invalid owner keypair length"));
    }
    let owner_pubkey_bytes = &owner_kp_bytes[32..64];
    let owner_pubkey_b58 = bs58::encode(owner_pubkey_bytes).into_string();

    // Check owner SOL balance, airdrop if needed (devnet)
    {
        let client = reqwest::Client::new();
        let resp: serde_json::Value = client
            .post(&rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getBalance",
                "params": [&owner_pubkey_b58]
            }))
            .send()
            .await?
            .json()
            .await?;
        let balance: u64 = resp
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        // If owner has less than needed (sol_funding + rent), request airdrop
        let needed = sol_funding + 5_000_000; // extra for rent + fees
        if balance < needed {
            let _ = devnet_airdrop(&rpc_url, &owner_pubkey_b58).await;
        }
    }

    // 1. Register on-chain
    let info = register_external_session_key(
        storage_path,
        rpc_url.clone(),
        resolved_owner_key.clone(),
        ephemeral_pubkey.clone(),
        ephemeral_secret_key.clone(),
        target_program,
        scopes.clone(),
        spending_limit,
        duration_secs,
        token_mint.clone(),
    )
    .await?;

    // 2. Fund the ephemeral key
    let _sigs = fund_session_key(
        rpc_url,
        resolved_owner_key,
        ephemeral_pubkey,
        sol_funding,
        token_mint,
        token_funding,
    )
    .await?;

    Ok(info)
}

/// Simple PDA derivation matching Solana's find_program_address.
/// Uses iterative nonce approach (255 down to 0) with SHA-256.
fn find_program_address_simple(seeds: &[&[u8]], program_id: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    const PDA_MARKER: &[u8] = b"ProgramDerivedAddress";

    for nonce in (0u8..=255u8).rev() {
        let mut hasher = Sha256::new();
        for seed in seeds {
            hasher.update(seed);
        }
        hasher.update(&[nonce]);
        hasher.update(program_id);
        hasher.update(PDA_MARKER);
        let hash: [u8; 32] = hasher.finalize().into();
        if !is_on_curve(&hash) {
            return hash;
        }
    }
    [0u8; 32]
}

/// Simple PDA derivation matching Solana's find_program_address.
/// Uses iterative nonce approach (255 down to 0) with SHA-256.
fn derive_session_pda_simple(owner: &[u8; 32], ephemeral: &[u8; 32]) -> [u8; 32] {
    let program_id = get_session_program_id_bytes();
    find_program_address_simple(&[b"session", owner, ephemeral], &program_id)
}

/// Check if a point is on the Ed25519 curve.
fn is_on_curve(point: &[u8; 32]) -> bool {
    use ed25519_dalek::VerifyingKey;
    VerifyingKey::from_bytes(point).is_ok()
}

/// Get the session program ID bytes.
fn get_session_program_id_bytes() -> [u8; 32] {
    bs58::decode("Avu35SYnvcSpWeYQhC7w2XT6DCurhnYB5PdajTqet9o")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap()
}

/// Get the system program ID bytes.
fn get_system_program_id_bytes() -> [u8; 32] {
    bs58::decode("11111111111111111111111111111111")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap()
}

/// Build the Anchor instruction data for register_session_key.
/// sighash(8) + target_program(32) + expires_at(8) + spending_limit(8) + scopes(borsh Vec<String>) + token_mint(32) + per_tx_limit(8) + daily_tx_count_limit(4)
fn build_register_ix_data(
    target_program: &[u8; 32],
    expires_at: i64,
    spending_limit: u64,
    scopes: &[String],
    token_mint: &[u8; 32],
    per_tx_limit: u64,
    daily_tx_count_limit: u32,
) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    // Anchor sighash: first 8 bytes of SHA-256("global:register_session_key")
    let sighash_preimage = b"global:register_session_key";
    let mut hasher = Sha256::new();
    hasher.update(sighash_preimage);
    let sighash = hasher.finalize();

    let mut data = Vec::new();
    data.extend_from_slice(&sighash[..8]);
    data.extend_from_slice(target_program);
    data.extend_from_slice(&expires_at.to_le_bytes());
    data.extend_from_slice(&spending_limit.to_le_bytes());

    // Borsh Vec<String>: u32 length + (u32 len + bytes) per string
    let scopes_len = scopes.len() as u32;
    data.extend_from_slice(&scopes_len.to_le_bytes());
    for scope in scopes {
        let scope_bytes = scope.as_bytes();
        let scope_len = scope_bytes.len() as u32;
        data.extend_from_slice(&scope_len.to_le_bytes());
        data.extend_from_slice(scope_bytes);
    }

    // token_mint: 32 bytes (Pubkey::default() for SOL sessions)
    data.extend_from_slice(token_mint);

    // per_tx_limit: 8 bytes LE (0 = no limit)
    data.extend_from_slice(&per_tx_limit.to_le_bytes());
    // daily_tx_count_limit: 4 bytes LE (0 = no limit)
    data.extend_from_slice(&daily_tx_count_limit.to_le_bytes());

    data
}

/// Get a recent blockhash via JSON-RPC.
async fn get_recent_blockhash(client: &reqwest::Client, rpc_url: &str) -> Result<String> {
    let resp: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "processed"}]
        }))
        .send()
        .await?
        .json()
        .await?;

    let blockhash = resp["result"]["value"]["blockhash"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No blockhash in response"))?;
    Ok(blockhash.to_string())
}

async fn account_exists(client: &reqwest::Client, rpc_url: &str, pubkey_b58: &str) -> Result<bool> {
    let resp: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAccountInfo",
            "params": [pubkey_b58, {"encoding": "base64"}]
        }))
        .send()
        .await?
        .json()
        .await?;
    if let Some(err) = resp.get("error") {
        return Err(anyhow::anyhow!("getAccountInfo failed: {}", err));
    }
    Ok(!resp["result"]["value"].is_null())
}

/// Build a raw signed register transaction (owner-only signature).
fn build_raw_transaction(
    owner_pubkey: &[u8; 32],
    owner_keypair: &[u8],
    ephemeral_pubkey: &[u8; 32],
    session_pda: &[u8; 32],
    target_program: &[u8; 32],
    ix_data: &[u8],
    blockhash: &str,
) -> Result<Vec<u8>> {
    use ed25519_dalek::{Signer, SigningKey};

    let blockhash_bytes = bs58::decode(blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    let message = build_register_message(
        owner_pubkey,
        ephemeral_pubkey,
        session_pda,
        target_program,
        ix_data,
        &blockhash_arr,
    )?;

    let owner_signing = SigningKey::from_bytes(&owner_keypair[..32].try_into().unwrap());
    let msg_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&message);
        let hash = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);
        arr
    };
    let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1);
    tx.extend_from_slice(&owner_sig);
    tx.extend_from_slice(&message);
    Ok(tx)
}

/// Legacy message layout for `register_session_key` (owner signs; ephemeral is pubkey-only).
fn build_register_message(
    owner_pubkey: &[u8; 32],
    ephemeral_pubkey: &[u8; 32],
    session_pda: &[u8; 32],
    target_program: &[u8; 32],
    ix_data: &[u8],
    blockhash_arr: &[u8; 32],
) -> Result<Vec<u8>> {
    let system_program: [u8; 32] = bs58::decode("11111111111111111111111111111111")
        .into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid system program id: {}", e))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid system program id length"))?;
    let clock_sysvar: [u8; 32] = bs58::decode("SysvarC1ock11111111111111111111111111111111")
        .into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid clock sysvar id: {}", e))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid clock sysvar id length"))?;
    let session_program_id = get_session_program_id_bytes();
    let account_keys: Vec<[u8; 32]> = vec![
        *owner_pubkey,
        *session_pda,
        *ephemeral_pubkey,
        session_program_id,
        *target_program,
        system_program,
        clock_sysvar,
    ];

    let mut message = Vec::new();
    message.push(1); // owner only
    message.push(0);
    message.push(5); // ephemeral + 4 readonly programs
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(blockhash_arr);
    compact_u64_encode(&mut message, 1);
    message.push(3); // session program
    let ix_accounts: Vec<u8> = vec![1, 0, 2, 4, 5, 6];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(ix_data);
    Ok(message)
}

/// Encode a u64 in Solana's compact-u16 format.
fn compact_u64_encode(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val > 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if val == 0 {
            break;
        }
    }
}

/// Send a raw transaction via JSON-RPC.
async fn send_transaction(
    client: &reqwest::Client,
    rpc_url: &str,
    tx_bytes: &[u8],
) -> Result<String> {
    let tx_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, tx_bytes);

    let resp: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                tx_b64,
                {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "confirmed",
                    "maxRetries": 5
                }
            ]
        }))
        .send()
        .await?
        .json()
        .await?;

    if let Some(error) = resp.get("error") {
        return Err(anyhow::anyhow!("{}", format_rpc_error(error)));
    }

    let signature = resp["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No signature in sendTransaction response"))?;
    wait_for_signature_confirmed(client, rpc_url, signature).await?;
    Ok(signature.to_string())
}

fn format_rpc_error(error: &serde_json::Value) -> String {
    let code = error
        .get("code")
        .and_then(|v| v.as_i64())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown RPC error");

    let data = error.get("data");
    let sim_err = data
        .and_then(|d| d.get("err"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string());
    let logs = data
        .and_then(|d| d.get("logs"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|line| line.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_else(|| "".to_string());

    if logs.is_empty() {
        format!("RPC error (code {}): {}; err={}", code, message, sim_err)
    } else {
        format!(
            "RPC error (code {}): {}; err={}; logs={}",
            code, message, sim_err, logs
        )
    }
}

async fn wait_for_signature_confirmed(
    client: &reqwest::Client,
    rpc_url: &str,
    signature: &str,
) -> Result<()> {
    for _ in 0..20 {
        let resp: serde_json::Value = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getSignatureStatuses",
                "params": [[signature], {"searchTransactionHistory": true}]
            }))
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = resp.get("error") {
            return Err(anyhow::anyhow!("getSignatureStatuses RPC error: {}", error));
        }

        if let Some(status) = resp["result"]["value"].get(0) {
            if status.is_null() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }

            if !status["err"].is_null() {
                return Err(anyhow::anyhow!(
                    "Transaction failed on-chain ({}): {}",
                    signature,
                    status["err"]
                ));
            }

            let confirmation = status["confirmationStatus"].as_str().unwrap_or_default();
            if confirmation == "confirmed" || confirmation == "finalized" {
                return Ok(());
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Err(anyhow::anyhow!(
        "Transaction not confirmed in time: {}",
        signature
    ))
}

// ── New public API: list, query, unsigned tx, complete, revoke, delete ────

/// List all session keys stored locally in sled.
/// Scans keys matching prefix `"session:"` and parses the stored value.
pub fn list_session_keys(storage_path: String) -> Result<Vec<SessionKeyEntry>> {
    let db = sled::open(&storage_path)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    let mut entries = Vec::new();
    let prefix = b"session:";
    for item in db.scan_prefix(prefix) {
        let (key, value) = item?;
        let pubkey_b58 = String::from_utf8_lossy(&key[prefix.len()..]).to_string();

        // Value layout: [64-byte keypair | 8-byte expires_at LE | 8-byte spending_limit LE | 8-byte per_tx_limit LE | 4-byte daily_tx_count_limit LE]
        if value.len() < 80 {
            continue;
        }
        let expires_at = i64::from_le_bytes(value[64..72].try_into().unwrap());
        let spending_limit = u64::from_le_bytes(value[72..80].try_into().unwrap());
        let per_tx_limit = if value.len() >= 88 {
            u64::from_le_bytes(value[80..88].try_into().unwrap())
        } else {
            0
        };
        let daily_tx_count_limit = if value.len() >= 92 {
            u32::from_le_bytes(value[88..92].try_into().unwrap())
        } else {
            0
        };

        let status = if expires_at < now {
            "expired".to_string()
        } else {
            "active".to_string()
        };

        entries.push(SessionKeyEntry {
            ephemeral_pubkey: pubkey_b58,
            expires_at,
            spending_limit,
            tx_signature: None, // not persisted locally yet
            session_pda: None,
            status,
            per_tx_limit,
            daily_tx_count_limit,
        });
    }

    Ok(entries)
}

/// Find the first active session key from local storage.
pub fn find_active_session_key(storage_path: String) -> Result<Option<SessionKeyEntry>> {
    let entries = list_session_keys(storage_path)?;
    Ok(entries.into_iter().find(|e| e.status == "active"))
}

/// Build an unsigned register-session-key transaction.
/// Stores the ephemeral keypair as `"pending:{pubkey}"` in sled for later retrieval.
/// Returns the unsigned tx bytes (base58), the PDA, and the ephemeral pubkey.
pub async fn build_unsigned_register_tx(
    storage_path: String,
    rpc_url: String,
    spending_limit: u64,
    duration_secs: i64,
) -> Result<UnsignedRegisterTx> {
    let _ = (storage_path, rpc_url, spending_limit, duration_secs);
    Err(anyhow::anyhow!(
        "Phone must not generate session keys; use build_register_tx_for_phantom with MCP pubkey"
    ))
}

/// Build an unsigned register-session-key transaction for an external wallet (WalletConnect).
/// Only the owner wallet signs; ephemeral pubkey is a non-signer account parameter.
pub async fn build_register_tx_for_phantom(
    storage_path: String,
    rpc_url: String,
    owner_pubkey_b58: String,
    ephemeral_pubkey_b58: String,
    target_program: String,
    scopes: Vec<String>,
    spending_limit: u64,
    duration_secs: i64,
    per_tx_limit: u64,
    daily_tx_count_limit: u32,
    token_mint: Option<String>,
) -> Result<UnsignedRegisterTx> {
    let db = sled::open(&storage_path)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let expires_at = now + duration_secs;

    // Parse owner pubkey (Phantom wallet)
    let owner_pubkey_bytes: [u8; 32] = bs58::decode(&owner_pubkey_b58)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid owner pubkey"))?;

    let ephemeral_pubkey_bytes: [u8; 32] = bs58::decode(&ephemeral_pubkey_b58)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid ephemeral pubkey"))?;

    // Derive session PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // Parse target program
    let target_program_bytes: [u8; 32] = bs58::decode(&target_program)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program"))?;

    // Parse token mint
    let token_mint_bytes: [u8; 32] = match &token_mint {
        Some(mint) => bs58::decode(mint)
            .into_vec()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid token mint"))?,
        None => [0u8; 32],
    };

    let ix_data = build_register_ix_data(
        &target_program_bytes,
        expires_at,
        spending_limit,
        &scopes,
        &token_mint_bytes,
        per_tx_limit,
        daily_tx_count_limit,
    );

    // Fetch blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    let message = build_register_message(
        &owner_pubkey_bytes,
        &ephemeral_pubkey_bytes,
        &session_pda,
        &target_program_bytes,
        &ix_data,
        &blockhash_arr,
    )?;

    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1);
    tx.extend_from_slice(&[0u8; 64]); // owner placeholder for wallet
    tx.extend_from_slice(&message);

    let unsigned_tx_b58 = bs58::encode(&tx).into_string();
    let session_pda_b58 = bs58::encode(&session_pda).into_string();

    // Store pending metadata (no secret on phone)
    let pending_key = format!("pending:{}", ephemeral_pubkey_b58);
    let mut pending_value = Vec::new();
    pending_value.extend_from_slice(&[0u8; 32]);
    pending_value.extend_from_slice(&expires_at.to_le_bytes());
    pending_value.extend_from_slice(&spending_limit.to_le_bytes());
    pending_value.extend_from_slice(&per_tx_limit.to_le_bytes());
    pending_value.extend_from_slice(&daily_tx_count_limit.to_le_bytes());
    db.insert(pending_key.as_bytes(), pending_value)?;

    Ok(UnsignedRegisterTx {
        unsigned_tx_b58,
        session_pda: session_pda_b58,
        ephemeral_pubkey: ephemeral_pubkey_b58,
    })
}

/// Broadcast a fully-signed base58-encoded transaction to the Solana cluster.
pub async fn broadcast_signed_tx(
    rpc_url: String,
    signed_tx_b58: String,
) -> Result<String> {
    let tx_bytes = bs58::decode(&signed_tx_b58).into_vec()?;
    let client = reqwest::Client::new();
    let signature = send_transaction(&client, &rpc_url, &tx_bytes).await?;
    Ok(signature)
}

/// Finalize a Phantom-signed session key registration.
/// Moves the key from `pending:{pubkey}` to `session:{pubkey}` in sled storage
/// after the transaction has been successfully broadcast.
///
/// Phone stores metadata only; ephemeral secret remains on MCP.
pub fn finalize_phantom_session_key(
    storage_path: String,
    ephemeral_pubkey: String,
    tx_signature: String,
    session_pda: String,
    _real_secret_key: Option<String>,
) -> Result<SessionKeyInfo> {
    let db = sled::open(&storage_path)?;

    let pending_key = format!("pending:{}", ephemeral_pubkey);
    let pending_value = db
        .remove(pending_key.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("No pending session key for {}", ephemeral_pubkey))?;

    // Parse pending value
    // Layout: [32-byte seed | 8-byte expires_at | 8-byte spending_limit | 8-byte per_tx_limit | 4-byte daily_tx_count_limit]
    if pending_value.len() < 60 {
        return Err(anyhow::anyhow!("Invalid pending session key data: {} bytes", pending_value.len()));
    }
    let expires_at = i64::from_le_bytes(pending_value[32..40].try_into().unwrap());
    let spending_limit = u64::from_le_bytes(pending_value[40..48].try_into().unwrap());
    let per_tx_limit = u64::from_le_bytes(pending_value[48..56].try_into().unwrap());
    let daily_tx_count_limit = u32::from_le_bytes(pending_value[56..60].try_into().unwrap());

    // Store permanently (64-byte secret slot = zeros; MCP holds the real key)
    let perm_key = format!("session:{}", ephemeral_pubkey);
    let mut perm_value = Vec::new();
    perm_value.extend_from_slice(&[0u8; 64]);
    perm_value.extend_from_slice(&expires_at.to_le_bytes());
    perm_value.extend_from_slice(&spending_limit.to_le_bytes());
    perm_value.extend_from_slice(&per_tx_limit.to_le_bytes());
    perm_value.extend_from_slice(&daily_tx_count_limit.to_le_bytes());
    db.insert(perm_key.as_bytes(), perm_value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey,
        ephemeral_secret_key: String::new(),
        expires_at,
        spending_limit,
        scopes: vec!["sol:transfer".to_string(), "spl:transfer".to_string()],
        tx_signature: Some(tx_signature),
        session_pda: Some(session_pda),
    })
}

/// Finalize an already-registered session key (pubkey-only local record; secret stays on MCP).
pub fn finalize_existing_session_key(
    storage_path: String,
    owner_pubkey_b58: String,
    ephemeral_pubkey: String,
    on_chain_info: SessionOnChainInfo,
    scopes: Vec<String>,
    _real_secret_key: Option<String>,
) -> Result<SessionKeyInfo> {
    // Decode owner and ephemeral pubkeys to derive session PDA
    let owner_bytes = bs58::decode(&owner_pubkey_b58).into_vec()?;
    if owner_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid owner pubkey length"));
    }
    let owner_arr: [u8; 32] = owner_bytes.try_into().unwrap();

    let ephemeral_bytes = bs58::decode(&ephemeral_pubkey).into_vec()?;
    if ephemeral_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid ephemeral pubkey length"));
    }
    let ephemeral_arr: [u8; 32] = ephemeral_bytes.try_into().unwrap();

    let session_pda = derive_session_pda_simple(&owner_arr, &ephemeral_arr);
    let session_pda_b58 = bs58::encode(&session_pda).into_string();

    let db = sled::open(&storage_path)?;
    let perm_key = format!("session:{}", ephemeral_pubkey);
    let mut perm_value = Vec::new();
    perm_value.extend_from_slice(&[0u8; 64]);
    perm_value.extend_from_slice(&on_chain_info.expires_at.to_le_bytes());
    perm_value.extend_from_slice(&on_chain_info.spending_limit.to_le_bytes());
    perm_value.extend_from_slice(&0u64.to_le_bytes()); // per_tx_limit: unknown, default 0
    perm_value.extend_from_slice(&0u32.to_le_bytes()); // daily_tx_count_limit: unknown, default 0
    db.insert(perm_key.as_bytes(), perm_value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey,
        ephemeral_secret_key: String::new(),
        expires_at: on_chain_info.expires_at,
        spending_limit: on_chain_info.spending_limit,
        scopes,
        tx_signature: None,
        session_pda: Some(session_pda_b58),
    })
}

/// Complete session key registration after receiving the owner signature from an external wallet.
/// Reconstructs the signed transaction, submits it, and moves the key from pending to permanent storage.
pub async fn complete_register_with_signature(
    storage_path: String,
    ephemeral_pubkey: String,
    owner_signature_b58: String,
    rpc_url: String,
) -> Result<SessionKeyInfo> {
    let db = sled::open(&storage_path)?;

    // Retrieve pending keypair
    let pending_key = format!("pending:{}", ephemeral_pubkey);
    let pending_value = db
        .remove(pending_key.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("No pending session key for {}", ephemeral_pubkey))?;

    if pending_value.len() < 60 {
        return Err(anyhow::anyhow!("Invalid pending session key data"));
    }
    let expires_at = i64::from_le_bytes(pending_value[32..40].try_into().unwrap());
    let spending_limit = u64::from_le_bytes(pending_value[40..48].try_into().unwrap());
    let per_tx_limit = u64::from_le_bytes(pending_value[48..56].try_into().unwrap());
    let daily_tx_count_limit = u32::from_le_bytes(pending_value[56..60].try_into().unwrap());

    let owner_sig_bytes: [u8; 64] = bs58::decode(&owner_signature_b58)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid owner signature length"))?;

    let ephemeral_pubkey_bytes: [u8; 32] = bs58::decode(&ephemeral_pubkey)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid ephemeral pubkey"))?;

    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed.as_slice().try_into().unwrap();
    let owner_pubkey_bytes =
        ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes)
            .verifying_key()
            .to_bytes();

    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    let target_program_bytes: [u8; 32] = bs58::decode("11111111111111111111111111111111")
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program"))?;
    let ix_data = build_register_ix_data(
        &target_program_bytes,
        expires_at,
        spending_limit,
        &["sol:transfer".to_string()],
        &[0u8; 32],
        per_tx_limit,
        daily_tx_count_limit,
    );

    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_arr: [u8; 32] = bs58::decode(&blockhash).into_vec()?.try_into().unwrap();

    let message = build_register_message(
        &owner_pubkey_bytes,
        &ephemeral_pubkey_bytes,
        &session_pda,
        &target_program_bytes,
        &ix_data,
        &blockhash_arr,
    )?;

    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1);
    tx.extend_from_slice(&owner_sig_bytes);
    tx.extend_from_slice(&message);

    let tx_signature = send_transaction(&client, &rpc_url, &tx).await?;

    let perm_key = format!("session:{}", ephemeral_pubkey);
    let mut perm_value = Vec::new();
    perm_value.extend_from_slice(&[0u8; 64]);
    perm_value.extend_from_slice(&expires_at.to_le_bytes());
    perm_value.extend_from_slice(&spending_limit.to_le_bytes());
    perm_value.extend_from_slice(&per_tx_limit.to_le_bytes());
    perm_value.extend_from_slice(&daily_tx_count_limit.to_le_bytes());
    db.insert(perm_key.as_bytes(), perm_value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey: ephemeral_pubkey.clone(),
        ephemeral_secret_key: String::new(),
        expires_at,
        spending_limit,
        scopes: vec!["sol:transfer".to_string()],
        tx_signature: Some(tx_signature),
        session_pda: Some(bs58::encode(&session_pda).into_string()),
    })
}

/// Revoke a session key on-chain by submitting a revoke_session instruction.
pub async fn revoke_session_key_onchain(
    storage_path: String,
    session_pubkey: String,
    rpc_url: String,
) -> Result<String> {
    let db = sled::open(&storage_path)?;

    // Retrieve session key data
    let key = format!("session:{}", session_pubkey);
    let value = db
        .get(key.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("Session key not found: {}", session_pubkey))?;
    if value.len() < 80 {
        return Err(anyhow::anyhow!("Invalid session key data"));
    }
    let ephemeral_pubkey_bytes: [u8; 32] = bs58::decode(&session_pubkey)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid session pubkey"))?;

    // Derive owner
    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed.as_slice().try_into().unwrap();
    let owner_signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();
    let _owner_keypair_bytes = owner_signing.to_bytes();

    // Derive PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // Build revoke instruction data: sighash of "global:revoke_session"
    let ix_data = {
        use sha2::{Digest, Sha256};
        let sighash_preimage = b"global:revoke_session";
        let mut hasher = Sha256::new();
        hasher.update(sighash_preimage);
        let sighash = hasher.finalize();
        sighash[..8].to_vec()
    };

    // Build revoke transaction
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    let session_program_id = get_session_program_id_bytes();
    // Accounts: [session_pda(writable), owner(signer, writable)]
    let account_keys: Vec<[u8; 32]> = vec![
        owner_pubkey_bytes,
        session_pda,
        session_program_id,
    ];

    let mut message = Vec::new();
    message.push(1); // num_required_signatures (owner only)
    message.push(0); // num_readonly_signed
    message.push(1); // num_readonly_unsigned (session_program)
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(&blockhash_arr);
    compact_u64_encode(&mut message, 1);
    message.push(2); // program_id_index = session_program_id
    let ix_accounts: Vec<u8> = vec![1, 0]; // [session_pda, owner]
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Sign
    use ed25519_dalek::Signer;
    let msg_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&message);
        let hash = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);
        arr
    };
    let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1);
    tx.extend_from_slice(&owner_sig);
    tx.extend_from_slice(&message);

    let tx_signature = send_transaction(&client, &rpc_url, &tx).await?;
    Ok(tx_signature)
}

/// Result of withdrawing funds from an old session key to a new one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawResult {
    /// SOL withdrawn in lamports.
    pub sol_withdrawn: u64,
    /// SPL token withdrawn in smallest unit.
    pub token_withdrawn: u64,
    /// SOL transfer transaction signature.
    pub sol_sig: Option<String>,
    /// Token transfer transaction signature.
    pub token_sig: Option<String>,
}

/// Withdraw SOL and optionally SPL tokens from an old session PDA to a new ephemeral key.
/// Uses the on-chain `withdraw_remaining` / `withdraw_spl_remaining` instructions,
/// signed by the owner (derived from DID). The old ephemeral key is used as fee payer.
pub async fn withdraw_session_funds(
    rpc_url: String,
    storage_path: String,
    old_ephemeral_pubkey: String,
    new_ephemeral_pubkey: String,
    token_mint_b58: Option<String>,
) -> Result<WithdrawResult> {
    let db = sled::open(&storage_path)?;

    // 1. Read old key's 64-byte secret key from sled (needed as fee payer)
    let key = format!("session:{}", old_ephemeral_pubkey);
    let value = db
        .get(key.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("Old session key not found: {}", old_ephemeral_pubkey))?;
    if value.len() < 64 {
        return Err(anyhow::anyhow!("Invalid session key data"));
    }
    let old_secret_64: [u8; 64] = value[..64].try_into().unwrap();

    // 2. Check that secret key is not all zeros
    if old_secret_64 == [0u8; 64] {
        return Err(anyhow::anyhow!("Cannot withdraw: old session key secret is zeros (MCP holds the key)"));
    }

    let old_signing = ed25519_dalek::SigningKey::from_bytes(&old_secret_64[..32].try_into().unwrap());
    let old_pubkey_bytes = old_signing.verifying_key().to_bytes();

    // Derive owner from DID
    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed.as_slice().try_into().unwrap();
    let owner_signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();
    let mut owner_keypair_bytes = owner_signing.to_bytes().to_vec();
    owner_keypair_bytes.extend_from_slice(&owner_pubkey_bytes);

    // Derive old session PDA
    let old_pda = derive_session_pda_simple(&owner_pubkey_bytes, &old_pubkey_bytes);

    let new_pubkey_bytes: [u8; 32] = bs58::decode(&new_ephemeral_pubkey)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid new ephemeral pubkey"))?;

    // Derive new session PDA (recipient for SOL withdrawal)
    let new_pda = derive_session_pda_simple(&owner_pubkey_bytes, &new_pubkey_bytes);

    let client = reqwest::Client::new();
    let mut result = WithdrawResult {
        sol_withdrawn: 0,
        token_withdrawn: 0,
        sol_sig: None,
        token_sig: None,
    };

    let session_program_id = get_session_program_id_bytes();

    // 3. Withdraw SOL from old PDA using on-chain withdraw_remaining instruction
    let pda_balance = get_sol_balance(rpc_url.clone(), bs58::encode(&old_pda).into_string()).await?;
    if pda_balance > 0 {
        // Build withdraw_remaining instruction data: sighash only (no amount param)
        let withdraw_sighash = {
            use sha2::{Digest, Sha256};
            let preimage = b"global:withdraw_remaining";
            let mut hasher = Sha256::new();
            hasher.update(preimage);
            hasher.finalize()[..8].to_vec()
        };

        let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
        let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
        let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

        // Accounts: [session_pda(writable), owner(signer,readonly), recipient(writable), system_program(readonly)]
        let system_program = get_system_program_id_bytes();

        // Fee payer is old ephemeral key (has some SOL for gas)
        let account_keys: Vec<[u8; 32]> = vec![
            old_pubkey_bytes,      // 0: fee payer (signer, writable)
            owner_pubkey_bytes,    // 1: owner (signer, readonly)
            old_pda,               // 2: session PDA (writable, non-signer)
            new_pda,               // 3: recipient (writable, non-signer)
            session_program_id,    // 4: session program (readonly)
            system_program,        // 5: system program (readonly)
        ];

        let mut message = Vec::new();
        message.push(2); // num_required_signatures (fee_payer + owner)
        message.push(1); // num_readonly_signed (owner is readonly+signer)
        message.push(3); // num_readonly_unsigned (session_program, system_program, ...)
        compact_u64_encode(&mut message, account_keys.len() as u64);
        for key in &account_keys {
            message.extend_from_slice(key);
        }
        message.extend_from_slice(&blockhash_arr);
        compact_u64_encode(&mut message, 1);
        message.push(4); // program_id_index = session_program
        let ix_accounts: Vec<u8> = vec![2, 1, 3, 5]; // [session_pda, owner, recipient, system_program]
        compact_u64_encode(&mut message, ix_accounts.len() as u64);
        message.extend_from_slice(&ix_accounts);
        compact_u64_encode(&mut message, withdraw_sighash.len() as u64);
        message.extend_from_slice(&withdraw_sighash);

        // Sign with both old ephemeral (fee payer) and owner
        use ed25519_dalek::Signer;
        let msg_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&message);
            let hash = hasher.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash);
            arr
        };
        let fee_payer_sig = old_signing.sign(&msg_hash).to_bytes();
        let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

        let mut tx = Vec::new();
        compact_u64_encode(&mut tx, 2);
        tx.extend_from_slice(&fee_payer_sig);
        tx.extend_from_slice(&owner_sig);
        tx.extend_from_slice(&message);

        let tx_sig = send_transaction(&client, &rpc_url, &tx).await?;
        result.sol_withdrawn = pda_balance;
        result.sol_sig = Some(tx_sig);
    }

    // 4. Withdraw SPL token from old PDA's ATA (if mint provided)
    if let Some(mint_b58) = token_mint_b58 {
        let mint_bytes: [u8; 32] = bs58::decode(&mint_b58)
            .into_vec()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid token mint"))?;

        let old_pda_b58 = bs58::encode(&old_pda).into_string();
        let token_balance = get_token_balance(
            rpc_url.clone(),
            old_pda_b58,
            mint_b58,
        )
        .await?;

        if token_balance > 0 {
            let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .into_vec()
                .unwrap()
                .try_into()
                .unwrap();

            let old_pda_ata = derive_ata(&old_pda, &mint_bytes);
            let new_pda_ata = derive_ata(&new_pda, &mint_bytes);

            // Build withdraw_spl_remaining instruction data: sighash + amount
            let withdraw_spl_sighash = {
                use sha2::{Digest, Sha256};
                let preimage = b"global:withdraw_spl_remaining";
                let mut hasher = Sha256::new();
                hasher.update(preimage);
                hasher.finalize()[..8].to_vec()
            };
            let mut withdraw_spl_data = Vec::new();
            withdraw_spl_data.extend_from_slice(&withdraw_spl_sighash);
            withdraw_spl_data.extend_from_slice(&token_balance.to_le_bytes());

            let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
            let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
            let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

            // Accounts: [session_pda(writable), owner(signer,readonly), source_ata(writable), dest_ata(writable), token_program(readonly)]
            let account_keys: Vec<[u8; 32]> = vec![
                old_pubkey_bytes,      // 0: fee payer (signer, writable)
                owner_pubkey_bytes,    // 1: owner (signer, readonly)
                old_pda,               // 2: session PDA (writable, non-signer)
                old_pda_ata,           // 3: source ATA (writable)
                new_pda_ata,           // 4: dest ATA (writable)
                session_program_id,    // 5: session program (readonly)
                token_program,         // 6: token program (readonly)
            ];

            let mut message = Vec::new();
            message.push(2); // num_required_signatures (fee_payer + owner)
            message.push(1); // num_readonly_signed (owner is readonly+signer)
            message.push(3); // num_readonly_unsigned
            compact_u64_encode(&mut message, account_keys.len() as u64);
            for key in &account_keys {
                message.extend_from_slice(key);
            }
            message.extend_from_slice(&blockhash_arr);
            compact_u64_encode(&mut message, 1);
            message.push(5); // program_id_index = session_program
            let ix_accounts: Vec<u8> = vec![2, 1, 3, 4, 6]; // [session_pda, owner, source_ata, dest_ata, token_program]
            compact_u64_encode(&mut message, ix_accounts.len() as u64);
            message.extend_from_slice(&ix_accounts);
            compact_u64_encode(&mut message, withdraw_spl_data.len() as u64);
            message.extend_from_slice(&withdraw_spl_data);

            use ed25519_dalek::Signer;
            let msg_hash = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&message);
                let hash = hasher.finalize();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&hash);
                arr
            };
            let fee_payer_sig = old_signing.sign(&msg_hash).to_bytes();
            let owner_sig = owner_signing.sign(&msg_hash).to_bytes();

            let mut tx = Vec::new();
            compact_u64_encode(&mut tx, 2);
            tx.extend_from_slice(&fee_payer_sig);
            tx.extend_from_slice(&owner_sig);
            tx.extend_from_slice(&message);

            let tx_sig = send_transaction(&client, &rpc_url, &tx).await?;
            result.token_withdrawn = token_balance;
            result.token_sig = Some(tx_sig);
        }
    }

    Ok(result)
}

/// Delete a session key from local sled storage only (no on-chain action).
pub fn delete_session_key_local(
    storage_path: String,
    session_pubkey: String,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let key = format!("session:{}", session_pubkey);
    db.remove(key.as_bytes())?;
    Ok(())
}

// ── Merchant Policy ──────────────────────────────────────────────────────

/// Per-merchant authorization policy stored locally in sled.
/// Key: `"policy:{merchant_did}"`, value: JSON-serialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantPolicy {
    pub merchant_did: String,
    /// Daily spending limit in lamports.
    pub daily_spending_limit: u64,
    /// Max number of transactions per day.
    pub daily_tx_count_limit: u32,
    /// Per-transaction spending limit in lamports.
    pub per_tx_limit: u64,
    /// Session duration in seconds.
    pub duration_secs: i64,
}

/// Save a merchant policy to sled.
pub fn save_merchant_policy(
    storage_path: String,
    merchant_did: String,
    daily_spending_limit: u64,
    daily_tx_count_limit: u32,
    per_tx_limit: u64,
    duration_secs: i64,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let policy = MerchantPolicy {
        merchant_did: merchant_did.clone(),
        daily_spending_limit,
        daily_tx_count_limit,
        per_tx_limit,
        duration_secs,
    };
    let key = format!("policy:{}", merchant_did);
    let value = serde_json::to_vec(&policy)?;
    db.insert(key.as_bytes(), value)?;
    Ok(())
}

/// Load a merchant policy from sled. Returns `None` if not found.
pub fn load_merchant_policy(
    storage_path: String,
    merchant_did: String,
) -> Result<Option<MerchantPolicy>> {
    let db = sled::open(&storage_path)?;
    let key = format!("policy:{}", merchant_did);
    match db.get(key.as_bytes())? {
        Some(value) => {
            let policy: MerchantPolicy = serde_json::from_slice(&value)?;
            Ok(Some(policy))
        }
        None => Ok(None),
    }
}

// ── Direct Wallet Payment: Build Unsigned SOL Transfer ──────────────────

/// Build an unsigned SOL transfer transaction for direct wallet signing.
///
/// Constructs a legacy Solana transaction with a SystemProgram Transfer instruction.
/// The first signature slot is filled with 64 zero bytes (placeholder) so that
/// the receiving wallet can replace it with the real signature.
///
/// The `merchant_did` parameter accepts either a DID string (`did:ignite:z...`)
/// or a raw base58-encoded Solana pubkey (for funding arbitrary addresses).
///
/// Returns the base58-encoded unsigned transaction bytes.
pub async fn build_unsigned_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_did: String,
    amount_lamports: u64,
) -> Result<String> {
    // Extract recipient Solana address from DID or raw base58 pubkey
    let merchant_pubkey = if merchant_did.starts_with("did:") {
        ignite_pay_core::identity::extract_pubkey_from_did(&merchant_did)
            .ok_or_else(|| anyhow::anyhow!("Cannot extract Solana pubkey from merchant DID: {}", merchant_did))?
    } else {
        // Treat as raw base58 pubkey (e.g. for funding session keys)
        let bytes = bs58::decode(&merchant_did).into_vec()?;
        if bytes.len() != 32 {
            return Err(anyhow::anyhow!("Invalid recipient pubkey length: expected 32, got {}", bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    };

    // 2. Decode wallet pubkey
    let wallet_pubkey = bs58::decode(&wallet_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid wallet pubkey base58"))?;
    if wallet_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Wallet pubkey must be 32 bytes"));
    }
    let wallet_pubkey_arr: [u8; 32] = wallet_pubkey.try_into().unwrap();

    // 3. Fetch recent blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // 4. System program address
    let system_program = get_system_program_id_bytes();

    // Account ordering:
    // 0: wallet (signer, writable)
    // 1: merchant (writable, non-signer)
    // 2: system_program (readonly, non-signer)
    let account_keys: Vec<[u8; 32]> = vec![
        wallet_pubkey_arr,
        merchant_pubkey,
        system_program,
    ];

    // Build message
    let mut message = Vec::new();
    message.push(1); // num_required_signatures = 1 (wallet)
    message.push(0); // num_readonly_signed = 0
    message.push(1); // num_readonly_unsigned = 1 (system_program)

    // Account keys compact-array
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    // Recent blockhash
    message.extend_from_slice(&blockhash_arr);

    // Instructions compact-array (1 instruction)
    compact_u64_encode(&mut message, 1);

    // Instruction 0: SystemProgram Transfer
    // program_id_index = 2 (system_program)
    message.push(2);

    // Account indices: [wallet(0), merchant(1)]
    let ix_accounts: Vec<u8> = vec![0, 1];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);

    // Transfer instruction data: 4-byte LE discriminant (2) + 8-byte LE amount = 12 bytes
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&2u32.to_le_bytes()); // SystemInstruction::Transfer
    ix_data.extend_from_slice(&amount_lamports.to_le_bytes());
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Build transaction: placeholder signature + message
    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1); // 1 signature
    tx.extend_from_slice(&[0u8; 64]); // placeholder signature
    tx.extend_from_slice(&message);

    Ok(bs58::encode(&tx).into_string())
}

// ── SPL Token Transfer ──────────────────────────────────────────────────

/// Derive the Associated Token Account address for a owner + mint pair.
/// ATA program ID: ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
/// Seeds: [owner, token_program, mint]
fn derive_ata(owner: &[u8; 32], mint: &[u8; 32]) -> [u8; 32] {
    let ata_program: [u8; 32] = bs58::decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();
    let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    find_program_address_simple(&[owner, &token_program, mint], &ata_program)
}

/// Build an unsigned SPL Token transfer transaction for direct wallet signing.
///
/// Constructs a legacy Solana transaction with a Token Program Transfer instruction.
/// Uses ATA derivation locally (no RPC needed for ATA lookup).
///
/// Account ordering:
/// 0: wallet (signer, writable)
/// 1: wallet_ata (writable, non-signer) — source ATA
/// 2: merchant_ata (writable, non-signer) — destination ATA
/// 3: token_program (readonly, non-signer)
pub async fn build_unsigned_spl_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_wallet_b58: String,
    amount: u64,
    token_mint_b58: String,
) -> Result<String> {
    // Decode keys
    let wallet_pubkey = bs58::decode(&wallet_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid wallet pubkey base58"))?;
    if wallet_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Wallet pubkey must be 32 bytes"));
    }
    let wallet_pubkey_arr: [u8; 32] = wallet_pubkey.try_into().unwrap();

    let merchant_pubkey = bs58::decode(&merchant_wallet_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid merchant wallet base58"))?;
    if merchant_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Merchant wallet must be 32 bytes"));
    }
    let merchant_pubkey_arr: [u8; 32] = merchant_pubkey.try_into().unwrap();

    let mint_bytes = bs58::decode(&token_mint_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid token mint base58"))?;
    if mint_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Token mint must be 32 bytes"));
    }
    let mint_arr: [u8; 32] = mint_bytes.try_into().unwrap();

    // Derive ATAs
    let wallet_ata = derive_ata(&wallet_pubkey_arr, &mint_arr);
    let merchant_ata = derive_ata(&merchant_pubkey_arr, &mint_arr);

    // Program addresses
    let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();
    let system_program = get_system_program_id_bytes();
    let ata_program: [u8; 32] = bs58::decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();
    let rent_sysvar: [u8; 32] = bs58::decode("SysvarRent111111111111111111111111111111111")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    // Fetch recent blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // Check whether destination ATA already exists.
    let merchant_ata_b58 = bs58::encode(&merchant_ata).into_string();
    let merchant_ata_exists = account_exists(&client, &rpc_url, &merchant_ata_b58).await?;

    // Account keys
    // 0: wallet (payer+authority signer, writable)
    // 1: wallet_ata (source ATA, writable)
    // 2: merchant_ata (destination ATA, writable)
    // 3: merchant wallet/PDA owner (readonly)
    // 4: mint (readonly)
    // 5: token program (readonly)
    // 6: system program (readonly)
    // 7: associated token account program (readonly)
    // 8: rent sysvar (readonly)
    let account_keys: Vec<[u8; 32]> = vec![
        wallet_pubkey_arr,
        wallet_ata,
        merchant_ata,
        merchant_pubkey_arr,
        mint_arr,
        token_program,
        system_program,
        ata_program,
        rent_sysvar,
    ];

    // Build message
    let mut message = Vec::new();
    message.push(1); // num_required_signatures = 1 (wallet)
    message.push(0); // num_readonly_signed = 0
    message.push(6); // num_readonly_unsigned = merchant, mint, token, system, ata_program, rent

    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    message.extend_from_slice(&blockhash_arr);

    if merchant_ata_exists {
        // 1 instruction: SPL token transfer
        compact_u64_encode(&mut message, 1);
    } else {
        // 2 instructions:
        //   1) createAssociatedTokenAccount (destination ATA)
        //   2) SPL token transfer
        compact_u64_encode(&mut message, 2);

        // Instruction 1: ATA create (legacy compatible)
        // Program: associated token program (index 7)
        message.push(7);
        // Accounts: [payer(0), ata(2), owner(3), mint(4), system(6), token(5), rent(8)]
        let create_ata_accounts: Vec<u8> = vec![0, 2, 3, 4, 6, 5, 8];
        compact_u64_encode(&mut message, create_ata_accounts.len() as u64);
        message.extend_from_slice(&create_ata_accounts);
        // Data: [] for Create
        compact_u64_encode(&mut message, 0);
    }

    // Final instruction: SPL Token Transfer
    // Program: token program (index 5)
    message.push(5);
    // Accounts: [source(1), dest(2), authority(0)]
    let transfer_accounts: Vec<u8> = vec![1, 2, 0];
    compact_u64_encode(&mut message, transfer_accounts.len() as u64);
    message.extend_from_slice(&transfer_accounts);
    // Data: Transfer = 1-byte discriminator (3) + 8-byte amount LE
    let mut ix_data = Vec::with_capacity(9);
    ix_data.push(3);
    ix_data.extend_from_slice(&amount.to_le_bytes());
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Build transaction: placeholder signature + message
    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 1);
    tx.extend_from_slice(&[0u8; 64]);
    tx.extend_from_slice(&message);

    Ok(bs58::encode(&tx).into_string())
}

/// Build an unsigned sponsored SPL Token transfer transaction for direct wallet signing.
///
/// Has 2 signature slots:
/// - slot 0: relayer (fee payer, placeholder)
/// - slot 1: wallet (signer, placeholder)
///
/// Account ordering:
/// 0: relayer (signer, writable — fee payer)
/// 1: wallet (signer, writable)
/// 2: wallet_ata (writable, non-signer) — source
/// 3: merchant_ata (writable, non-signer) — dest
/// 4: token_program (readonly, non-signer)
pub async fn build_unsigned_sponsored_spl_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_wallet_b58: String,
    amount: u64,
    token_mint_b58: String,
    relayer_pubkey_b58: String,
) -> Result<String> {
    // Decode keys
    let wallet_pubkey = bs58::decode(&wallet_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid wallet pubkey base58"))?;
    if wallet_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Wallet pubkey must be 32 bytes"));
    }
    let wallet_pubkey_arr: [u8; 32] = wallet_pubkey.try_into().unwrap();

    let merchant_pubkey = bs58::decode(&merchant_wallet_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid merchant wallet base58"))?;
    if merchant_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Merchant wallet must be 32 bytes"));
    }
    let merchant_pubkey_arr: [u8; 32] = merchant_pubkey.try_into().unwrap();

    let relayer_pubkey = bs58::decode(&relayer_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid relayer pubkey base58"))?;
    if relayer_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Relayer pubkey must be 32 bytes"));
    }
    let relayer_pubkey_arr: [u8; 32] = relayer_pubkey.try_into().unwrap();

    let mint_bytes = bs58::decode(&token_mint_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid token mint base58"))?;
    if mint_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Token mint must be 32 bytes"));
    }
    let mint_arr: [u8; 32] = mint_bytes.try_into().unwrap();

    // Derive ATAs
    let wallet_ata = derive_ata(&wallet_pubkey_arr, &mint_arr);
    let merchant_ata = derive_ata(&merchant_pubkey_arr, &mint_arr);

    // Token program
    let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    // Fetch blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // Account keys
    let account_keys: Vec<[u8; 32]> = vec![
        relayer_pubkey_arr, // 0: relayer (signer, writable — fee payer)
        wallet_pubkey_arr,  // 1: wallet (signer, writable)
        wallet_ata,         // 2: source ATA (writable)
        merchant_ata,       // 3: dest ATA (writable)
        token_program,      // 4: token program (readonly)
    ];

    let mut message = Vec::new();
    message.push(2); // num_required_signatures = 2 (relayer + wallet)
    message.push(0); // num_readonly_signed = 0
    message.push(1); // num_readonly_unsigned = 1 (token_program)

    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    message.extend_from_slice(&blockhash_arr);

    compact_u64_encode(&mut message, 1);

    // program_id_index = 4 (token_program)
    message.push(4);

    // Account indices: [source(2), dest(3), authority(1)]
    let ix_accounts: Vec<u8> = vec![2, 3, 1];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);

    // SPL Token program instruction layout:
    //   Transfer = 1-byte discriminator (3) + 8-byte amount LE
    let mut ix_data = Vec::with_capacity(9);
    ix_data.push(3); // Transfer
    ix_data.extend_from_slice(&amount.to_le_bytes());
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Build transaction: 2 placeholder signatures + message
    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 2);
    tx.extend_from_slice(&[0u8; 64]); // placeholder for relayer signature
    tx.extend_from_slice(&[0u8; 64]); // placeholder for wallet signature
    tx.extend_from_slice(&message);

    Ok(bs58::encode(&tx).into_string())
}

// ── Balance, On-Chain Check, Payment Records ───────────────────────────

/// On-chain session key account info parsed from the PDA data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOnChainInfo {
    pub exists: bool,
    pub spending_limit: u64,
    pub current_spent: u64,
    pub revoked: bool,
    pub expires_at: i64,
}

/// A persisted payment authorization record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub payment_id: String,
    pub merchant_did: String,
    pub amount: u64,
    pub token_mint: Option<String>,
    pub description: String,
    pub authorized: bool,
    pub timestamp: i64,
    pub session_key_pubkey: Option<String>,
    pub tx_signature: Option<String>,
}

/// A transaction history entry from getSignaturesForAddress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxHistoryEntry {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
}

/// Get SOL balance (in lamports) for a base58-encoded pubkey via JSON-RPC.
pub async fn get_sol_balance(rpc_url: String, pubkey_b58: String) -> Result<u64> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(&rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [&pubkey_b58]
        }))
        .send()
        .await?
        .json()
        .await?;
    if let Some(err) = resp.get("error") {
        return Err(anyhow::anyhow!("getBalance failed: {}", err));
    }
    let balance = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(balance)
}

/// Get SPL token balance for an owner + mint pair via JSON-RPC.
/// Derives the ATA locally, returns 0 if the ATA doesn't exist.
pub async fn get_token_balance(
    rpc_url: String,
    owner_pubkey_b58: String,
    token_mint_b58: String,
) -> Result<u64> {
    let owner_bytes = bs58::decode(&owner_pubkey_b58).into_vec()?;
    if owner_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid owner pubkey length"));
    }
    let owner_arr: [u8; 32] = owner_bytes.try_into().unwrap();

    let mint_bytes = bs58::decode(&token_mint_b58).into_vec()?;
    if mint_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid token mint length"));
    }
    let mint_arr: [u8; 32] = mint_bytes.try_into().unwrap();

    let ata = derive_ata(&owner_arr, &mint_arr);
    let ata_b58 = bs58::encode(&ata).into_string();

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(&rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountBalance",
            "params": [&ata_b58]
        }))
        .send()
        .await?
        .json()
        .await?;

    // Account doesn't exist or has no balance
    if resp.get("error").is_some() {
        return Ok(0);
    }
    let amount = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("amount"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    Ok(amount)
}

/// Check if a session key PDA exists on-chain and parse its account data.
pub async fn get_session_account_info(
    rpc_url: String,
    owner_b58: String,
    ephemeral_b58: String,
) -> Result<SessionOnChainInfo> {
    let owner_bytes = bs58::decode(&owner_b58).into_vec()?;
    if owner_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid owner pubkey"));
    }
    let owner_arr: [u8; 32] = owner_bytes.try_into().unwrap();

    let ephemeral_bytes = bs58::decode(&ephemeral_b58).into_vec()?;
    if ephemeral_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid ephemeral pubkey"));
    }
    let ephemeral_arr: [u8; 32] = ephemeral_bytes.try_into().unwrap();

    let pda = derive_session_pda_simple(&owner_arr, &ephemeral_arr);
    let pda_b58 = bs58::encode(&pda).into_string();

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(&rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAccountInfo",
            "params": [&pda_b58, {"encoding": "base64"}]
        }))
        .send()
        .await?
        .json()
        .await?;

    let data_b64 = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("data"))
        .and_then(|d| d.get(0))
        .and_then(|s| s.as_str());

    match data_b64 {
        None => Ok(SessionOnChainInfo {
            exists: false,
            spending_limit: 0,
            current_spent: 0,
            revoked: false,
            expires_at: 0,
        }),
        Some(b64) => {
            let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)?;
            // Borsh layout after 8-byte Anchor discriminator:
            // [32 owner] [32 ephemeral] [32 target_program] [32 token_mint]
            // [8 expires_at] [8 spending_limit] [8 current_spent]
            // [variable scopes] [8 per_tx_limit] [4 daily_tx_count_limit]
            // [4 current_daily_count] [8 last_daily_reset] [1 revoked] [1 bump]
            let disc = 8;
            if data.len() < disc + 128 + 24 {
                return Ok(SessionOnChainInfo {
                    exists: true,
                    spending_limit: 0,
                    current_spent: 0,
                    revoked: false,
                    expires_at: 0,
                });
            }
            let expires_at = i64::from_le_bytes(data[disc + 128..disc + 136].try_into().unwrap());
            let spending_limit =
                u64::from_le_bytes(data[disc + 136..disc + 144].try_into().unwrap());
            let current_spent =
                u64::from_le_bytes(data[disc + 144..disc + 152].try_into().unwrap());

            // Parse scopes to find where the fixed fields after scopes begin
            let scopes_start = disc + 152;
            let scope_count =
                u32::from_le_bytes(data[scopes_start..scopes_start + 4].try_into().unwrap()) as usize;
            let mut offset = scopes_start + 4;
            for _ in 0..scope_count {
                if offset + 4 > data.len() {
                    break;
                }
                let slen =
                    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4 + slen;
            }

            // After scopes: [8 per_tx_limit] [4 daily_tx_count_limit] [4 current_daily_count]
            //               [8 last_daily_reset] [1 revoked] [1 bump]
            let mut revoked = false;
            offset += 8 + 4 + 4 + 8; // skip per_tx_limit, daily_count, current_daily, last_daily_reset
            if offset < data.len() {
                revoked = data[offset] != 0;
            }

            Ok(SessionOnChainInfo {
                exists: true,
                spending_limit,
                current_spent,
                revoked,
                expires_at,
            })
        }
    }
}

/// Derive the owner's Solana pubkey from the DID stored in sled.
/// Reuses the pattern: get_did() → sha256(did) → SigningKey → verifying_key → bs58.
pub fn get_owner_pubkey(storage_path: String) -> Result<String> {
    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
    let signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let pubkey = signing.verifying_key();
    Ok(bs58::encode(pubkey.to_bytes()).into_string())
}

/// Save a payment authorization record to sled.
/// Key format: `"payrec:{timestamp}:{payment_id}"`.
pub fn save_payment_record(storage_path: String, record: PaymentRecord) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let key = format!("payrec:{}:{}", record.timestamp, record.payment_id);
    let value = serde_json::to_vec(&record)?;
    db.insert(key.as_bytes(), value)?;
    Ok(())
}

/// List all payment records from sled, newest-first.
pub fn list_payment_records(storage_path: String) -> Result<Vec<PaymentRecord>> {
    let db = sled::open(&storage_path)?;
    let mut records = Vec::new();
    for item in db.scan_prefix(b"payrec:") {
        let (_, value) = item?;
        if let Ok(record) = serde_json::from_slice::<PaymentRecord>(&value) {
            records.push(record);
        }
    }
    // Reverse for newest-first (keys are timestamp-ordered ascending)
    records.reverse();
    Ok(records)
}

/// Get recent transaction signatures for a pubkey via JSON-RPC getSignaturesForAddress.
pub async fn get_transaction_history(
    rpc_url: String,
    pubkey_b58: String,
    limit: u32,
) -> Result<Vec<TxHistoryEntry>> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post(&rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignaturesForAddress",
            "params": [&pubkey_b58, {"limit": limit}]
        }))
        .send()
        .await?
        .json()
        .await?;

    if let Some(err) = resp.get("error") {
        return Err(anyhow::anyhow!("getSignaturesForAddress failed: {}", err));
    }

    let entries = resp
        .get("result")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(TxHistoryEntry {
                        signature: v.get("signature")?.as_str()?.to_string(),
                        slot: v.get("slot")?.as_u64()?,
                        block_time: v.get("blockTime").and_then(|bt| bt.as_i64()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(entries)
}

// ── Sponsored (Relayer) Payment ─────────────────────────────────────────

/// Fetch the relayer's fee-payer public key from GET /info.
pub async fn fetch_relayer_pubkey(relayer_url: String) -> Result<String> {
    let info_url = format!("{}/info", relayer_url.trim_end_matches('/'));
    let resp: serde_json::Value = reqwest::get(&info_url).await?.json().await?;
    let pubkey = resp["pubkey"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'pubkey' in relayer /info response"))?;
    Ok(pubkey.to_string())
}

/// Build an unsigned sponsored SOL transfer transaction for direct wallet signing.
///
/// Unlike `build_unsigned_transfer_tx`, this has 2 signature slots:
/// - slot 0: relayer (fee payer, placeholder — relayer will sign)
/// - slot 1: wallet (signer, placeholder — wallet will sign via signTransaction)
///
/// Account ordering:
/// 0: relayer (signer, writable — fee payer)
/// 1: wallet (signer, writable)
/// 2: merchant (writable, non-signer)
/// 3: system_program (readonly, non-signer)
pub async fn build_unsigned_sponsored_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_did: String,
    amount_lamports: u64,
    relayer_pubkey_b58: String,
) -> Result<String> {
    // 1. Extract merchant Solana address from DID
    let merchant_pubkey = ignite_pay_core::identity::extract_pubkey_from_did(&merchant_did)
        .ok_or_else(|| anyhow::anyhow!("Cannot extract Solana pubkey from merchant DID: {}", merchant_did))?;

    // 2. Decode wallet pubkey
    let wallet_pubkey = bs58::decode(&wallet_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid wallet pubkey base58"))?;
    if wallet_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Wallet pubkey must be 32 bytes"));
    }
    let wallet_pubkey_arr: [u8; 32] = wallet_pubkey.try_into().unwrap();

    // 3. Decode relayer pubkey
    let relayer_pubkey = bs58::decode(&relayer_pubkey_b58)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("Invalid relayer pubkey base58"))?;
    if relayer_pubkey.len() != 32 {
        return Err(anyhow::anyhow!("Relayer pubkey must be 32 bytes"));
    }
    let relayer_pubkey_arr: [u8; 32] = relayer_pubkey.try_into().unwrap();

    // 4. Fetch recent blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // 5. System program address
    let system_program = get_system_program_id_bytes();

    // Account ordering:
    // 0: relayer (signer, writable — fee payer)
    // 1: wallet (signer, writable)
    // 2: merchant (writable, non-signer)
    // 3: system_program (readonly, non-signer)
    let account_keys: Vec<[u8; 32]> = vec![
        relayer_pubkey_arr,
        wallet_pubkey_arr,
        merchant_pubkey,
        system_program,
    ];

    // Build message
    let mut message = Vec::new();
    message.push(2); // num_required_signatures = 2 (relayer + wallet)
    message.push(0); // num_readonly_signed = 0
    message.push(1); // num_readonly_unsigned = 1 (system_program)

    // Account keys compact-array
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    // Recent blockhash
    message.extend_from_slice(&blockhash_arr);

    // Instructions compact-array (1 instruction)
    compact_u64_encode(&mut message, 1);

    // Instruction 0: SystemProgram Transfer
    // program_id_index = 3 (system_program)
    message.push(3);

    // Account indices: [wallet(1), merchant(2)]
    let ix_accounts: Vec<u8> = vec![1, 2];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);

    // Transfer instruction data: 4-byte LE discriminant (2) + 8-byte LE amount = 12 bytes
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&2u32.to_le_bytes()); // SystemInstruction::Transfer
    ix_data.extend_from_slice(&amount_lamports.to_le_bytes());
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Build transaction: 2 placeholder signatures + message
    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 2); // 2 signatures
    tx.extend_from_slice(&[0u8; 64]); // placeholder for relayer signature
    tx.extend_from_slice(&[0u8; 64]); // placeholder for wallet signature
    tx.extend_from_slice(&message);

    Ok(bs58::encode(&tx).into_string())
}
