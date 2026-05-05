use anyhow::Result;
use ignite_pay_mb_sdk::{pda, signing};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

/// Verify a buyer's MB voucher signature.
pub fn verify_mb_voucher(
    program_id: String,
    buyer_pubkey: String,
    merchant_pubkey: String,
    seq: u64,
    amount: u64,
    buyer_sig: String,
) -> Result<bool> {
    let program_id: Pubkey = program_id.parse()
        .map_err(|e| anyhow::anyhow!("Invalid program_id: {}", e))?;
    let buyer: Pubkey = buyer_pubkey.parse()
        .map_err(|e| anyhow::anyhow!("Invalid buyer_pubkey: {}", e))?;
    let merchant: Pubkey = merchant_pubkey.parse()
        .map_err(|e| anyhow::anyhow!("Invalid merchant_pubkey: {}", e))?;

    let token_mint = Pubkey::default(); // SOL
    let (channel_pda, _) = pda::derive_channel_pda(&program_id, &buyer, &merchant, &token_mint);
    let channel_id = channel_pda.to_bytes();

    // Decode buyer signature
    let sig_bytes = bs58::decode(&buyer_sig).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid buyer_sig base58: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err(anyhow::anyhow!("buyer_sig must be 64 bytes"));
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);

    // Compute msg_hash the same way as sign_voucher
    let msg_hash = {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&channel_id);
        hasher.update(&seq.to_be_bytes());
        hasher.update(&amount.to_be_bytes());
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hasher.finalize());
        hash
    };

    Ok(signing::verify_signature(&buyer.to_bytes(), &msg_hash, &sig_arr))
}

/// Store a verified MB voucher in sled.
pub fn store_mb_voucher(
    storage_path: String,
    channel_id: String,
    buyer_pubkey: String,
    seq: u64,
    amount: u64,
    buyer_sig: String,
) -> Result<()> {
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("merchant_mb_vouchers")?;

    let channel_bytes = bs58::decode(&channel_id).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid channel_id: {}", e))?;
    let _buyer_bytes = Pubkey::try_from(buyer_pubkey.as_str())
        .map_err(|e| anyhow::anyhow!("Invalid buyer_pubkey: {}", e))?;
    let sig_bytes = bs58::decode(&buyer_sig).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid buyer_sig: {}", e))?;

    let mut channel_arr = [0u8; 32];
    channel_arr.copy_from_slice(&channel_bytes);
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);

    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(&channel_arr);
    key.extend_from_slice(&seq.to_be_bytes());

    let voucher_data = serde_json::json!({
        "channel_id": channel_id,
        "buyer": buyer_pubkey,
        "seq": seq,
        "amount": amount,
        "buyer_sig": buyer_sig,
    });

    tree.insert(&key, serde_json::to_vec(&voucher_data)?)?;
    tree.flush()?;
    Ok(())
}

/// Initialize or load MB merchant keypair from sled, return base58 pubkey.
pub fn initialize_mb_merchant(storage_path: String) -> Result<String> {
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("mb_merchant")?;

    let kp = match tree.get("keypair")? {
        Some(bytes) if bytes.len() == 64 => {
            Keypair::try_from(bytes.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to load MB keypair: {}", e))?
        }
        _ => {
            let kp = Keypair::new();
            tree.insert("keypair", kp.to_bytes().as_ref())?;
            tree.flush()?;
            kp
        }
    };

    Ok(kp.pubkey().to_string())
}

/// Get the MB merchant pubkey from storage.
pub fn get_mb_merchant_pubkey(storage_path: String) -> Result<String> {
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("mb_merchant")?;

    match tree.get("keypair")? {
        Some(bytes) if bytes.len() == 64 => {
            let kp = Keypair::try_from(bytes.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to load MB keypair: {}", e))?;
            Ok(kp.pubkey().to_string())
        }
        _ => Err(anyhow::anyhow!("No MB merchant keypair found")),
    }
}
