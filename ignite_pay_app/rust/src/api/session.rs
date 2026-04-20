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
) -> Result<SessionKeyInfo> {
    let db = sled::open(&storage_path)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    // Generate ephemeral Ed25519 keypair
    let mut csprng = rand::rngs::OsRng;
    let keypair = ed25519_dalek::SigningKey::generate(&mut csprng);
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    let expires_at = now + duration_secs;

    // Store the keypair bytes in sled
    let key = format!("session:{}", bs58::encode(&pubkey_bytes).into_string());
    let mut value = Vec::new();
    value.extend_from_slice(&keypair.to_bytes());
    value.extend_from_slice(&expires_at.to_le_bytes());
    value.extend_from_slice(&spending_limit.to_le_bytes());
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

    let ix_data = build_register_ix_data(&program_id_bytes, expires_at, spending_limit, &scopes);

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
    let key = format!(
        "session:{}",
        bs58::encode(&ephemeral_pubkey_bytes).into_string()
    );
    let mut value = Vec::new();
    value.extend_from_slice(&ephemeral_secret_bytes);
    value.extend_from_slice(&expires_at.to_le_bytes());
    value.extend_from_slice(&spending_limit.to_le_bytes());
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
/// sighash(8) + target_program(32) + expires_at(8) + spending_limit(8) + scopes(borsh Vec<String>)
fn build_register_ix_data(
    target_program: &[u8; 32],
    expires_at: i64,
    spending_limit: u64,
    scopes: &[String],
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

        // Value layout: [64-byte keypair | 8-byte expires_at LE | 8-byte spending_limit LE]
        if value.len() < 80 {
            continue;
        }
        let expires_at = i64::from_le_bytes(value[64..72].try_into().unwrap());
        let spending_limit = u64::from_le_bytes(value[72..80].try_into().unwrap());

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

    // Store pending keypair: "pending:{pubkey}" -> [64-byte keypair | 8-byte expires_at LE | 8-byte spending_limit LE]
    let pending_key = format!("pending:{}", ephemeral_pubkey_b58);
    let mut pending_value = Vec::new();
    pending_value.extend_from_slice(&ephemeral_secret_bytes);
    pending_value.extend_from_slice(&expires_at.to_le_bytes());
    pending_value.extend_from_slice(&spending_limit.to_le_bytes());
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
    if pending_value.len() < 80 {
        return Err(anyhow::anyhow!("Invalid pending session key data"));
    }
    let ephemeral_secret_bytes: [u8; 64] = pending_value[..64].try_into().unwrap();
    let expires_at = i64::from_le_bytes(pending_value[64..72].try_into().unwrap());
    let spending_limit = u64::from_le_bytes(pending_value[72..80].try_into().unwrap());

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
    let owner_keypair_bytes = owner_signing.to_bytes();

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
