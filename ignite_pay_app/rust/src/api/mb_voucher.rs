use anyhow::Result;
use ignite_pay_mb_sdk::{pda, signing};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

/// Result of signing an MB voucher.
pub struct MbVoucherResult {
    pub channel_id: String,
    pub seq: u64,
    pub amount: u64,
    pub buyer_pubkey: String,
    pub buyer_sig: String,
}

/// Get or generate the buyer MB keypair, return base58 pubkey.
pub fn get_mb_buyer_pubkey(storage_path: String) -> Result<String> {
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("mb_buyer")?;

    let kp = match tree.get("keypair")? {
        Some(bytes) if bytes.len() == 64 => {
            Keypair::try_from(bytes.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to load MB buyer keypair: {}", e))?
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

/// Get the next voucher seq for a given channel.
pub fn get_next_voucher_seq(storage_path: String, channel_id: String) -> Result<u64> {
    let db = sled::open(&storage_path)?;
    let tree = db.open_tree("mb_vouchers")?;

    let channel_bytes = bs58::decode(&channel_id).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid channel_id: {}", e))?;
    let mut max_seq: u64 = 0;
    for item in tree.scan_prefix(&channel_bytes) {
        let (key, _) = item?;
        if key.len() >= 40 {
            let seq_bytes: [u8; 8] = key[32..40].try_into()
                .map_err(|_| anyhow::anyhow!("Invalid key format"))?;
            let seq = u64::from_be_bytes(seq_bytes);
            if seq > max_seq {
                max_seq = seq;
            }
        }
    }

    Ok(max_seq + 1)
}

/// Sign an MB voucher.
/// 1. Load buyer MB keypair from sled
/// 2. Derive channel PDA
/// 3. Sign voucher
/// 4. Store signed voucher in sled
pub fn sign_mb_voucher(
    storage_path: String,
    program_id: String,
    merchant_mb_pubkey: String,
    seq: u64,
    amount: u64,
) -> Result<MbVoucherResult> {
    let db = sled::open(&storage_path)?;
    let buyer_tree = db.open_tree("mb_buyer")?;
    let voucher_tree = db.open_tree("mb_vouchers")?;

    // Load buyer keypair
    let kp_bytes = buyer_tree.get("keypair")?
        .ok_or_else(|| anyhow::anyhow!("No MB buyer keypair found. Call get_mb_buyer_pubkey first."))?;
    let kp = Keypair::try_from(kp_bytes.as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to load MB buyer keypair: {}", e))?;
    let buyer_pubkey = kp.pubkey().to_string();

    // Parse keys
    let program_id_pubkey: Pubkey = program_id.parse()
        .map_err(|e| anyhow::anyhow!("Invalid program_id: {}", e))?;
    let merchant_pubkey: Pubkey = merchant_mb_pubkey.parse()
        .map_err(|e| anyhow::anyhow!("Invalid merchant_mb_pubkey: {}", e))?;

    // Derive channel PDA
    let token_mint = Pubkey::default(); // SOL
    let (channel_pda, _) = pda::derive_channel_pda(
        &program_id_pubkey,
        &kp.pubkey(),
        &merchant_pubkey,
        &token_mint,
    );
    let channel_id = channel_pda.to_bytes();

    // Sign voucher
    let kp_bytes_64 = kp.to_bytes();
    let (msg_hash, sig) = signing::sign_voucher(&channel_id, seq, amount, &kp_bytes_64);
    let buyer_sig = bs58::encode(sig).into_string();

    // Store voucher
    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(&channel_id);
    key.extend_from_slice(&seq.to_be_bytes());

    let voucher_data = serde_json::json!({
        "channel_id": channel_pda.to_string(),
        "buyer_pubkey": buyer_pubkey,
        "seq": seq,
        "amount": amount,
        "buyer_sig": buyer_sig,
        "msg_hash": bs58::encode(msg_hash).into_string(),
    });

    voucher_tree.insert(&key, serde_json::to_vec(&voucher_data)?)?;
    voucher_tree.flush()?;

    Ok(MbVoucherResult {
        channel_id: channel_pda.to_string(),
        seq,
        amount,
        buyer_pubkey,
        buyer_sig,
    })
}

/// Build a JWE-encrypted mb-voucher DIDComm message.
/// Returns the JWE string ready to be sent via WS.
pub async fn build_mb_voucher_jwe(
    storage_path: String,
    merchant_did: String,
    order_id: String,
    channel_id: String,
    seq: u64,
    amount: u64,
    buyer_pubkey: String,
    buyer_sig: String,
) -> Result<String> {
    use ignite_pay_core::didcomm;
    use crate::api::identity::IdentityManager;

    let mgr = IdentityManager::new(&storage_path)?;
    let our_did = mgr.did().to_string();
    let agent = mgr.agent();

    // Build the mb-voucher DIDComm message
    let msg = serde_json::json!({
        "type": "https://didcomm.org/ignite-pay/1.0/mb-voucher",
        "id": format!("mb-voucher-{}", uuid::Uuid::new_v4()),
        "from": our_did,
        "to": [merchant_did],
        "body": {
            "order_id": order_id,
            "channel_id": channel_id,
            "seq": seq,
            "amount": amount,
            "buyer_pubkey": buyer_pubkey,
            "buyer_sig": buyer_sig,
        }
    });

    let didcomm_msg = affinidi_messaging_didcomm::Message::from_json(
        serde_json::to_string(&msg)?.as_bytes(),
    ).map_err(|e| anyhow::anyhow!("Failed to build DIDComm message: {}", e))?;

    let agent_guard = agent.lock().await;
    let jwe = didcomm::pack_encrypted(&agent_guard, &didcomm_msg, &our_did, &merchant_did)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    Ok(jwe)
}
