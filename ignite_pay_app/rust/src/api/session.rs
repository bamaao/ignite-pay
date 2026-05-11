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
use sha2::Digest;
use std::time::{SystemTime, UNIX_EPOCH};

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
/// This generates an ephemeral Ed25519 keypair locally and stores it in sled.
/// Returns the session key info without requiring on-chain operations.
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
    let db = sled::open(&storage_path)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    // Generate ephemeral Ed25519 keypair
    let mut csprng = rand::rngs::OsRng;
    let keypair = ed25519_dalek::SigningKey::generate(&mut csprng);
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    let expires_at = now + duration_secs;

    // Store the keypair bytes in sled
    // Layout: [64-byte keypair | 8-byte expires_at LE | 8-byte spending_limit LE | 8-byte per_tx_limit LE | 4-byte daily_tx_count_limit LE]
    let key = format!("session:{}", bs58::encode(&pubkey_bytes).into_string());
    let mut value = Vec::new();
    value.extend_from_slice(&keypair.to_bytes());
    value.extend_from_slice(&expires_at.to_le_bytes());
    value.extend_from_slice(&spending_limit.to_le_bytes());
    value.extend_from_slice(&per_tx_limit.to_le_bytes());
    value.extend_from_slice(&daily_tx_count_limit.to_le_bytes());
    db.insert(key.as_bytes(), value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey: bs58::encode(&pubkey_bytes).into_string(),
        ephemeral_secret_key: bs58::encode(&keypair.to_bytes()).into_string(),
        expires_at,
        spending_limit,
        scopes,
        tx_signature: None,
        session_pda: None,
    })
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
    let db = sled::open(&storage_path)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    let expires_at = now + duration_secs;

    // 1. Generate ephemeral keypair
    let mut csprng = rand::rngs::OsRng;
    let ephemeral_signing = ed25519_dalek::SigningKey::generate(&mut csprng);
    let ephemeral_pubkey = ephemeral_signing.verifying_key();
    let ephemeral_pubkey_bytes = ephemeral_pubkey.to_bytes();
    let ephemeral_secret_bytes = ephemeral_signing.to_bytes();

    // 2. Decode owner keypair
    let owner_keypair_bytes = bs58::decode(&owner_secret_key).into_vec()?;
    if owner_keypair_bytes.len() != 64 {
        return Err(anyhow::anyhow!("Invalid owner keypair length"));
    }
    let owner_signing =
        ed25519_dalek::SigningKey::from_bytes(&owner_keypair_bytes[..32].try_into().unwrap());
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();

    // 3. Derive session PDA
    // PDA seeds: ["session", owner.as_ref(), ephemeral.as_ref()]
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // 4. Build Anchor instruction data
    let program_id_bytes: [u8; 32] = bs58::decode(&target_program)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program ID"))?;

    let ix_data = build_register_ix_data(&program_id_bytes, expires_at, spending_limit, &scopes, &[0u8; 32], 0, 0);

    // 5. Build raw transaction via JSON-RPC
    let client = reqwest::Client::new();

    // Get recent blockhash
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;

    // Build transaction
    let tx_bytes = build_raw_transaction(
        &owner_pubkey_bytes,
        &owner_keypair_bytes,
        &ephemeral_pubkey_bytes,
        &ephemeral_secret_bytes,
        &session_pda,
        &program_id_bytes,
        &ix_data,
        &blockhash,
    )?;

    // 6. Submit via JSON-RPC
    let tx_signature = send_transaction(&client, &rpc_url, &tx_bytes).await?;

    // Store locally in sled
    // Layout: [64-byte keypair | 8-byte expires_at | 8-byte spending_limit | 8-byte per_tx_limit | 4-byte daily_tx_count_limit]
    let key = format!(
        "session:{}",
        bs58::encode(&ephemeral_pubkey_bytes).into_string()
    );
    let mut value = Vec::new();
    value.extend_from_slice(&ephemeral_secret_bytes);
    value.extend_from_slice(&expires_at.to_le_bytes());
    value.extend_from_slice(&spending_limit.to_le_bytes());
    value.extend_from_slice(&0u64.to_le_bytes()); // per_tx_limit
    value.extend_from_slice(&0u32.to_le_bytes()); // daily_tx_count_limit
    db.insert(key.as_bytes(), value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey: bs58::encode(&ephemeral_pubkey_bytes).into_string(),
        ephemeral_secret_key: bs58::encode(&ephemeral_secret_bytes).into_string(),
        expires_at,
        spending_limit,
        scopes,
        tx_signature: Some(tx_signature),
        session_pda: Some(bs58::encode(&session_pda).into_string()),
    })
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
    let db = sled::open(&storage_path)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let expires_at = now + duration_secs;

    // 1. Decode the externally-provided ephemeral keypair
    let ephemeral_secret_bytes = bs58::decode(&ephemeral_secret_key).into_vec()?;
    if ephemeral_secret_bytes.len() != 64 {
        return Err(anyhow::anyhow!("Invalid ephemeral keypair length: expected 64 bytes, got {}", ephemeral_secret_bytes.len()));
    }
    let ephemeral_signing = ed25519_dalek::SigningKey::from_bytes(&ephemeral_secret_bytes[..32].try_into().unwrap());
    let ephemeral_pubkey_bytes = ephemeral_signing.verifying_key().to_bytes();

    // Verify pubkey matches
    let expected_pubkey_bytes = bs58::decode(&ephemeral_pubkey).into_vec()?;
    if ephemeral_pubkey_bytes[..] != expected_pubkey_bytes[..] {
        return Err(anyhow::anyhow!("Ephemeral pubkey mismatch"));
    }

    // 2. Decode owner keypair (derive from DID if empty)
    let (owner_keypair_bytes, owner_signing) = if owner_secret_key.is_empty() {
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
        (kp_bytes, signing)
    } else {
        let kp_bytes = bs58::decode(&owner_secret_key).into_vec()?;
        if kp_bytes.len() != 64 {
            return Err(anyhow::anyhow!("Invalid owner keypair length"));
        }
        let signing =
            ed25519_dalek::SigningKey::from_bytes(&kp_bytes[..32].try_into().unwrap());
        (kp_bytes, signing)
    };
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();

    // 3. Derive session PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // 4. Build Anchor instruction data
    let program_id_bytes: [u8; 32] = bs58::decode(&target_program)
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program ID"))?;
    let token_mint_bytes: [u8; 32] = match &token_mint {
        Some(mint) => bs58::decode(mint)
            .into_vec()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid token mint"))?,
        None => [0u8; 32],
    };
    let ix_data = build_register_ix_data(&program_id_bytes, expires_at, spending_limit, &scopes, &token_mint_bytes, 0, 0);

    // 5. Build raw transaction via JSON-RPC
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;

    let tx_bytes = build_raw_transaction(
        &owner_pubkey_bytes,
        &owner_keypair_bytes,
        &ephemeral_pubkey_bytes,
        &ephemeral_secret_bytes,
        &session_pda,
        &program_id_bytes,
        &ix_data,
        &blockhash,
    )?;

    // 6. Submit via JSON-RPC
    let tx_signature = send_transaction(&client, &rpc_url, &tx_bytes).await?;

    // Store locally in sled
    // Layout: [64-byte keypair | 8-byte expires_at | 8-byte spending_limit | 8-byte per_tx_limit | 4-byte daily_tx_count_limit]
    let key = format!(
        "session:{}",
        bs58::encode(&ephemeral_pubkey_bytes).into_string()
    );
    let mut value = Vec::new();
    value.extend_from_slice(&ephemeral_secret_bytes);
    value.extend_from_slice(&expires_at.to_le_bytes());
    value.extend_from_slice(&spending_limit.to_le_bytes());
    value.extend_from_slice(&0u64.to_le_bytes()); // per_tx_limit
    value.extend_from_slice(&0u32.to_le_bytes()); // daily_tx_count_limit
    db.insert(key.as_bytes(), value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey: bs58::encode(&ephemeral_pubkey_bytes).into_string(),
        ephemeral_secret_key: bs58::encode(&ephemeral_secret_bytes).into_string(),
        expires_at,
        spending_limit,
        scopes,
        tx_signature: Some(tx_signature),
        session_pda: Some(bs58::encode(&session_pda).into_string()),
    })
}

/// Fund a session key by transferring SOL (and optionally SPL token) from the owner.
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

    // Decode recipient pubkey
    let recipient_bytes = bs58::decode(&ephemeral_pubkey).into_vec()?;
    if recipient_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid recipient pubkey"));
    }
    let recipient_pubkey: [u8; 32] = recipient_bytes.try_into().unwrap();

    // System program
    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];

    // --- SOL transfer ---
    if sol_amount > 0 {
        let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
        let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
        let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

        let account_keys: Vec<[u8; 32]> = vec![
            owner_pubkey_bytes,
            recipient_pubkey,
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
        let ix_accounts: Vec<u8> = vec![0, 1]; // [owner, recipient]
        compact_u64_encode(&mut message, ix_accounts.len() as u64);
        message.extend_from_slice(&ix_accounts);
        let mut ix_data = Vec::with_capacity(12);
        ix_data.extend_from_slice(&0u32.to_le_bytes()); // Transfer discriminant
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

    // --- SPL token transfer (optional) ---
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

            // Derive ATAs
            let owner_ata = derive_ata(&owner_pubkey_bytes, &mint_bytes);
            let recipient_ata = derive_ata(&recipient_pubkey, &mint_bytes);

            let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
            let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
            let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

            let account_keys: Vec<[u8; 32]> = vec![
                owner_pubkey_bytes,
                owner_ata,
                recipient_ata,
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
            let mut ix_data = Vec::with_capacity(12);
            ix_data.extend_from_slice(&3u32.to_le_bytes()); // Token Transfer discriminant
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
fn derive_session_pda_simple(owner: &[u8; 32], ephemeral: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    let program_id = get_session_program_id_bytes();

    for nonce in (0u8..=255u8).rev() {
        let mut hasher = Sha256::new();
        hasher.update(b"session");
        hasher.update(owner);
        hasher.update(ephemeral);
        hasher.update(&[nonce]);
        hasher.update(&program_id);
        let hash = hasher.finalize().into();

        // Check if it's off-curve (not a valid Ed25519 point)
        // For simplicity, return the first valid one
        if !is_on_curve(&hash) {
            return hash;
        }
    }

    // Fallback (should never happen)
    [0u8; 32]
}

/// Check if a point is on the Ed25519 curve.
fn is_on_curve(point: &[u8; 32]) -> bool {
    use ed25519_dalek::VerifyingKey;
    VerifyingKey::from_bytes(point).is_ok()
}

/// Get the session program ID bytes.
fn get_session_program_id_bytes() -> [u8; 32] {
    bs58::decode("6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei")
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
            "params": [{"commitment": "finalized"}]
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

/// Build a raw signed transaction in Solana's compact-array format.
fn build_raw_transaction(
    owner_pubkey: &[u8; 32],
    owner_keypair: &[u8],
    ephemeral_pubkey: &[u8; 32],
    ephemeral_secret: &[u8],
    session_pda: &[u8; 32],
    program_id: &[u8; 32],
    ix_data: &[u8],
    blockhash: &str,
) -> Result<Vec<u8>> {
    use ed25519_dalek::{Signer, SigningKey};

    // Decode blockhash from base58 to 32 bytes
    let blockhash_bytes = bs58::decode(blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // System program and Clock sysvar addresses (hardcoded)
    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];
    let clock_sysvar: [u8; 32] = [
        0x06, 0xa7, 0xd5, 0xde, 0x18, 0x4a, 0x62, 0xa4, 0x54, 0xd2, 0x8d, 0x8c, 0xf2, 0xf4, 0xdc,
        0xb2, 0x3d, 0x50, 0x25, 0x6b, 0x3e, 0xfb, 0x75, 0xbf, 0x15, 0xbe, 0x6e, 0x2a, 0xb1, 0xc8,
        0x91, 0x24,
    ];

    // Account ordering (Solana legacy message format):
    // 0: owner (signer, writable)
    // 1: ephemeral_signer (signer, readonly)
    // 2: session_pda (writable, non-signer)
    // 3: session_program_id (readonly, non-signer) — the program being called
    // 4: target_program (readonly, non-signer) — instruction parameter
    // 5: system_program (readonly, non-signer)
    // 6: clock_sysvar (readonly, non-signer)
    let session_program_id = get_session_program_id_bytes();
    let account_keys: Vec<[u8; 32]> = vec![
        *owner_pubkey,
        *ephemeral_pubkey,
        *session_pda,
        session_program_id,
        *program_id,
        system_program,
        clock_sysvar,
    ];

    let mut message = Vec::new();

    // Header
    message.push(2); // num_required_signatures = 2 (owner, ephemeral)
    message.push(1); // num_readonly_signed = 1 (ephemeral is readonly+signer)
    message.push(4); // num_readonly_unsigned = 4 (session_program, target_program, system_program, clock)

    // Account keys compact-array
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    // Recent blockhash
    message.extend_from_slice(&blockhash_arr);

    // Instructions compact-array (1 instruction)
    compact_u64_encode(&mut message, 1);

    // Instruction 0: program_id_index = 3 (session_program_id)
    message.push(3);

    // Account indices for register_session_key:
    // [session_pda(2), owner(0), ephemeral(1), target_program(4), system_program(5), clock(6)]
    let ix_accounts: Vec<u8> = vec![2, 0, 1, 4, 5, 6];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);

    // Instruction data
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(ix_data);

    // Sign the message
    let owner_signing = SigningKey::from_bytes(&owner_keypair[..32].try_into().unwrap());
    let ephemeral_signing = SigningKey::from_bytes(&ephemeral_secret[..32].try_into().unwrap());

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
    let ephemeral_sig = ephemeral_signing.sign(&msg_hash).to_bytes();

    // Build transaction: signatures + message
    let mut tx = Vec::new();

    // Signatures compact-array (2 signatures)
    compact_u64_encode(&mut tx, 2);
    tx.extend_from_slice(&owner_sig);
    tx.extend_from_slice(&ephemeral_sig);

    // Message
    tx.extend_from_slice(&message);

    Ok(tx)
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
                {"encoding": "base64", "skipPreflight": true}
            ]
        }))
        .send()
        .await?
        .json()
        .await?;

    if let Some(error) = resp.get("error") {
        return Err(anyhow::anyhow!("RPC error: {}", error));
    }

    let signature = resp["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No signature in sendTransaction response"))?;
    Ok(signature.to_string())
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
    let db = sled::open(&storage_path)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let expires_at = now + duration_secs;

    // Generate ephemeral keypair
    let mut csprng = rand::rngs::OsRng;
    let ephemeral_signing = ed25519_dalek::SigningKey::generate(&mut csprng);
    let ephemeral_pubkey = ephemeral_signing.verifying_key();
    let ephemeral_pubkey_bytes = ephemeral_pubkey.to_bytes();
    let ephemeral_secret_bytes = ephemeral_signing.to_bytes();

    // Derive owner from DID (same as create_session_key_for_payment)
    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
    let owner_signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();

    // Derive session PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // Build instruction data (target_program = system program)
    let target_program_bytes: [u8; 32] = bs58::decode("11111111111111111111111111111111")
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program"))?;
    let ix_data = build_register_ix_data(
        &target_program_bytes,
        expires_at,
        spending_limit,
        &["sol:transfer".to_string()],
        &[0u8; 32], // SOL session: default Pubkey
        0, // per_tx_limit: 0 = no limit
        0, // daily_tx_count_limit: 0 = no limit
    );

    // Fetch blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;

    // Build unsigned transaction message (same layout as build_raw_transaction, minus signatures)
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];
    let clock_sysvar: [u8; 32] = [
        0x06, 0xa7, 0xd5, 0xde, 0x18, 0x4a, 0x62, 0xa4, 0x54, 0xd2, 0x8d, 0x8c, 0xf2, 0xf4, 0xdc,
        0xb2, 0x3d, 0x50, 0x25, 0x6b, 0x3e, 0xfb, 0x75, 0xbf, 0x15, 0xbe, 0x6e, 0x2a, 0xb1, 0xc8,
        0x91, 0x24,
    ];

    let session_program_id = get_session_program_id_bytes();
    let account_keys: Vec<[u8; 32]> = vec![
        owner_pubkey_bytes,
        ephemeral_pubkey_bytes,
        session_pda,
        session_program_id,
        target_program_bytes,
        system_program,
        clock_sysvar,
    ];

    let mut message = Vec::new();
    message.push(2); // num_required_signatures
    message.push(1); // num_readonly_signed
    message.push(4); // num_readonly_unsigned
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(&blockhash_arr);
    compact_u64_encode(&mut message, 1); // 1 instruction
    message.push(3); // program_id_index
    let ix_accounts: Vec<u8> = vec![2, 0, 1, 4, 5, 6];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Sign the message with the ephemeral key (owner slot left as zeros for external signing)
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
    let ephemeral_sig = ephemeral_signing.sign(&msg_hash).to_bytes();

    // Build transaction with placeholder owner sig (64 zero bytes) + ephemeral sig
    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 2);
    tx.extend_from_slice(&[0u8; 64]); // placeholder owner signature
    tx.extend_from_slice(&ephemeral_sig);
    tx.extend_from_slice(&message);

    let unsigned_tx_b58 = bs58::encode(&tx).into_string();
    let ephemeral_pubkey_b58 = bs58::encode(&ephemeral_pubkey_bytes).into_string();
    let session_pda_b58 = bs58::encode(&session_pda).into_string();

    // Store pending keypair: "pending:{pubkey}" -> [64-byte keypair | 8-byte expires_at | 8-byte spending_limit | 8-byte per_tx_limit | 4-byte daily_tx_count_limit]
    let pending_key = format!("pending:{}", ephemeral_pubkey_b58);
    let mut pending_value = Vec::new();
    pending_value.extend_from_slice(&ephemeral_secret_bytes);
    pending_value.extend_from_slice(&expires_at.to_le_bytes());
    pending_value.extend_from_slice(&spending_limit.to_le_bytes());
    pending_value.extend_from_slice(&0u64.to_le_bytes()); // per_tx_limit
    pending_value.extend_from_slice(&0u32.to_le_bytes()); // daily_tx_count_limit
    db.insert(pending_key.as_bytes(), pending_value)?;

    Ok(UnsignedRegisterTx {
        unsigned_tx_b58,
        session_pda: session_pda_b58,
        ephemeral_pubkey: ephemeral_pubkey_b58,
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

    // Parse pending value
    // Layout: [64-byte keypair | 8-byte expires_at | 8-byte spending_limit | 8-byte per_tx_limit | 4-byte daily_tx_count_limit]
    if pending_value.len() < 92 {
        return Err(anyhow::anyhow!("Invalid pending session key data"));
    }
    let ephemeral_secret_bytes: [u8; 64] = pending_value[..64].try_into().unwrap();
    let expires_at = i64::from_le_bytes(pending_value[64..72].try_into().unwrap());
    let spending_limit = u64::from_le_bytes(pending_value[72..80].try_into().unwrap());
    let _per_tx_limit = u64::from_le_bytes(pending_value[80..88].try_into().unwrap());
    let _daily_tx_count_limit = u32::from_le_bytes(pending_value[88..92].try_into().unwrap());

    // Decode owner signature
    let owner_sig_bytes = bs58::decode(&owner_signature_b58).into_vec()?;
    if owner_sig_bytes.len() != 64 {
        return Err(anyhow::anyhow!("Invalid owner signature length"));
    }

    // Re-derive the owner and ephemeral pubkeys to rebuild the message
    let ephemeral_signing = ed25519_dalek::SigningKey::from_bytes(&ephemeral_secret_bytes[..32].try_into().unwrap());
    let ephemeral_pubkey_bytes = ephemeral_signing.verifying_key().to_bytes();

    let identity_mgr = crate::api::identity::IdentityManager::new(&storage_path)?;
    let did = identity_mgr.did();
    let owner_seed = sha2::Sha256::digest(did.as_bytes());
    let owner_seed_bytes: &[u8; 32] = owner_seed.as_slice().try_into().unwrap();
    let owner_signing = ed25519_dalek::SigningKey::from_bytes(owner_seed_bytes);
    let owner_pubkey_bytes = owner_signing.verifying_key().to_bytes();

    // Derive session PDA
    let session_pda = derive_session_pda_simple(&owner_pubkey_bytes, &ephemeral_pubkey_bytes);

    // Build instruction data
    let target_program_bytes: [u8; 32] = bs58::decode("11111111111111111111111111111111")
        .into_vec()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid target program"))?;
    let ix_data = build_register_ix_data(
        &target_program_bytes,
        expires_at,
        spending_limit,
        &["sol:transfer".to_string()],
        &[0u8; 32], // SOL session: default Pubkey
        0, // per_tx_limit: 0 = no limit
        0, // daily_tx_count_limit: 0 = no limit
    );

    // Fetch fresh blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];
    let clock_sysvar: [u8; 32] = [
        0x06, 0xa7, 0xd5, 0xde, 0x18, 0x4a, 0x62, 0xa4, 0x54, 0xd2, 0x8d, 0x8c, 0xf2, 0xf4, 0xdc,
        0xb2, 0x3d, 0x50, 0x25, 0x6b, 0x3e, 0xfb, 0x75, 0xbf, 0x15, 0xbe, 0x6e, 0x2a, 0xb1, 0xc8,
        0x91, 0x24,
    ];
    let session_program_id = get_session_program_id_bytes();
    let account_keys: Vec<[u8; 32]> = vec![
        owner_pubkey_bytes,
        ephemeral_pubkey_bytes,
        session_pda,
        session_program_id,
        target_program_bytes,
        system_program,
        clock_sysvar,
    ];

    let mut message = Vec::new();
    message.push(2);
    message.push(1);
    message.push(4);
    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(&blockhash_arr);
    compact_u64_encode(&mut message, 1);
    message.push(3);
    let ix_accounts: Vec<u8> = vec![2, 0, 1, 4, 5, 6];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);
    compact_u64_encode(&mut message, ix_data.len() as u64);
    message.extend_from_slice(&ix_data);

    // Sign with both keys
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
    let ephemeral_sig = ephemeral_signing.sign(&msg_hash).to_bytes();

    let mut tx = Vec::new();
    compact_u64_encode(&mut tx, 2);
    tx.extend_from_slice(&owner_sig);
    tx.extend_from_slice(&ephemeral_sig);
    tx.extend_from_slice(&message);

    // Submit
    let tx_signature = send_transaction(&client, &rpc_url, &tx).await?;

    // Store permanently
    let perm_key = format!("session:{}", ephemeral_pubkey);
    let mut perm_value = Vec::new();
    perm_value.extend_from_slice(&ephemeral_secret_bytes);
    perm_value.extend_from_slice(&expires_at.to_le_bytes());
    perm_value.extend_from_slice(&spending_limit.to_le_bytes());
    perm_value.extend_from_slice(&_per_tx_limit.to_le_bytes());
    perm_value.extend_from_slice(&_daily_tx_count_limit.to_le_bytes());
    db.insert(perm_key.as_bytes(), perm_value)?;

    Ok(SessionKeyInfo {
        ephemeral_pubkey: ephemeral_pubkey.clone(),
        ephemeral_secret_key: bs58::encode(&ephemeral_secret_bytes).into_string(),
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
    let ephemeral_secret_bytes: [u8; 64] = value[..64].try_into().unwrap();
    let ephemeral_signing = ed25519_dalek::SigningKey::from_bytes(&ephemeral_secret_bytes[..32].try_into().unwrap());
    let ephemeral_pubkey_bytes = ephemeral_signing.verifying_key().to_bytes();

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
/// Returns the base58-encoded unsigned transaction bytes.
pub async fn build_unsigned_transfer_tx(
    rpc_url: String,
    wallet_pubkey_b58: String,
    merchant_did: String,
    amount_lamports: u64,
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

    // 3. Fetch recent blockhash
    let client = reqwest::Client::new();
    let blockhash = get_recent_blockhash(&client, &rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash).into_vec()?;
    if blockhash_bytes.len() != 32 {
        return Err(anyhow::anyhow!("Invalid blockhash"));
    }
    let blockhash_arr: [u8; 32] = blockhash_bytes.try_into().unwrap();

    // 4. System program address
    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];

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

    // Transfer instruction data: 4-byte LE discriminant (0) + 8-byte LE amount = 12 bytes
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&0u32.to_le_bytes()); // Transfer discriminant
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

    for nonce in (0u8..=255u8).rev() {
        let mut hasher = sha2::Sha256::new();
        hasher.update(owner);
        hasher.update(&token_program);
        hasher.update(mint);
        hasher.update(&ata_program);
        hasher.update(&[nonce]);
        let hash = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);
        if !is_on_curve(&arr) {
            return arr;
        }
    }
    [0u8; 32]
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

    // Token program address
    let token_program: [u8; 32] = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
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

    // Account keys
    let account_keys: Vec<[u8; 32]> = vec![
        wallet_pubkey_arr,  // 0: wallet (signer, writable)
        wallet_ata,         // 1: source ATA (writable)
        merchant_ata,       // 2: dest ATA (writable)
        token_program,      // 3: token program (readonly)
    ];

    // Build message
    let mut message = Vec::new();
    message.push(1); // num_required_signatures = 1 (wallet)
    message.push(0); // num_readonly_signed = 0
    message.push(1); // num_readonly_unsigned = 1 (token_program)

    compact_u64_encode(&mut message, account_keys.len() as u64);
    for key in &account_keys {
        message.extend_from_slice(key);
    }

    message.extend_from_slice(&blockhash_arr);

    // 1 instruction: Token Transfer
    compact_u64_encode(&mut message, 1);

    // program_id_index = 3 (token_program)
    message.push(3);

    // Account indices: [source(1), dest(2), authority(0)]
    let ix_accounts: Vec<u8> = vec![1, 2, 0];
    compact_u64_encode(&mut message, ix_accounts.len() as u64);
    message.extend_from_slice(&ix_accounts);

    // Token Transfer instruction data: 1-byte discriminant (12) + 8-byte LE amount = 9 bytes
    // Note: SPL Token Transfer discriminant is 12 (from anchor-discriminator style, but actually
    // the Token program uses: 1st byte = instruction index. Transfer = 3 for Token program.
    // Actually: Token Program instruction layout:
    //   Transfer: 4-byte LE discriminant (3) + 8-byte LE amount
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&3u32.to_le_bytes()); // Transfer discriminant
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

    // Token Transfer instruction data
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&3u32.to_le_bytes());
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
    let system_program: [u8; 32] = [
        0x06, 0x9b, 0x88, 0x64, 0xd1, 0x6a, 0xed, 0x71, 0x48, 0xb5, 0xd0, 0x40, 0xb1, 0x3e, 0xa0,
        0x17, 0x42, 0xaf, 0x28, 0x37, 0xa0, 0xc8, 0x72, 0x21, 0x53, 0x25, 0x04, 0xb2, 0x5d, 0x2d,
        0x5e, 0x06,
    ];

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

    // Transfer instruction data: 4-byte LE discriminant (0) + 8-byte LE amount = 12 bytes
    let mut ix_data = Vec::with_capacity(12);
    ix_data.extend_from_slice(&0u32.to_le_bytes()); // Transfer discriminant
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
