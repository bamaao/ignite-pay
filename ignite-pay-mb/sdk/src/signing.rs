use ed25519_dalek::{SigningKey, Signer, VerifyingKey, Verifier, Signature};
use sha2::{Sha256, Digest};

/// Build the settlement message hash:
/// `SHA256(merkle_root || amount_le || channel_id || nonce_le)`
pub fn build_settlement_message(
    merkle_root: &[u8; 32],
    total_amount: u64,
    channel_id: &[u8; 32],
    nonce: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(merkle_root);
    hasher.update(&total_amount.to_le_bytes());
    hasher.update(channel_id);
    hasher.update(&nonce.to_le_bytes());
    let mut result = [0u8; 32];
    result.copy_from_slice(&hasher.finalize());
    result
}

/// Sign a settlement message with an Ed25519 secret key.
/// `secret_key` is the 64-byte keypair (first 32 bytes = secret, last 32 = public).
pub fn sign_settlement(msg_hash: &[u8; 32], secret_key: &[u8; 64]) -> [u8; 64] {
    let signing_key = SigningKey::from_bytes(&secret_key[..32].try_into().unwrap());
    let signature = signing_key.sign(msg_hash);
    signature.to_bytes()
}

/// Build and sign a payment voucher:
/// `SHA256(channel_id || seq_le || amount_le)` then sign.
/// `secret_key` is the 64-byte keypair (first 32 bytes = secret, last 32 = public).
pub fn sign_voucher(
    channel_id: &[u8; 32],
    seq: u64,
    amount: u64,
    secret_key: &[u8; 64],
) -> ([u8; 32], [u8; 64]) {
    let mut hasher = Sha256::new();
    hasher.update(channel_id);
    hasher.update(&seq.to_le_bytes());
    hasher.update(&amount.to_le_bytes());
    let mut msg_hash = [0u8; 32];
    msg_hash.copy_from_slice(&hasher.finalize());

    let signing_key = SigningKey::from_bytes(&secret_key[..32].try_into().unwrap());
    let signature = signing_key.sign(&msg_hash);
    (msg_hash, signature.to_bytes())
}

/// Verify an Ed25519 signature.
pub fn verify_signature(
    pubkey: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> bool {
    let verifying_key = match VerifyingKey::from_bytes(pubkey) {
        Ok(vk) => vk,
        Err(_) => return false,
    };
    let sig = Signature::from_bytes(signature);
    verifying_key.verify(message, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    /// Create a deterministic keypair from a seed byte.
    fn make_keypair(seed: u8) -> ([u8; 64], [u8; 32]) {
        let secret_bytes = [seed; 32];
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let verifying_key = signing_key.verifying_key();
        let mut kp_bytes = [0u8; 64];
        kp_bytes[..32].copy_from_slice(&secret_bytes);
        kp_bytes[32..].copy_from_slice(verifying_key.as_bytes());
        (kp_bytes, verifying_key.to_bytes())
    }

    #[test]
    fn test_sign_and_verify_voucher() {
        let (secret, pubkey) = make_keypair(0x01);
        let channel_id = [0x11; 32];

        let (msg_hash, sig) = sign_voucher(&channel_id, 42, 1000, &secret);

        assert!(verify_signature(&pubkey, &msg_hash, &sig));
    }

    #[test]
    fn test_verify_wrong_pubkey_fails() {
        let (secret, _pubkey) = make_keypair(0x01);
        let (_other_secret, other_pubkey) = make_keypair(0x02);
        let channel_id = [0x11; 32];

        let (msg_hash, sig) = sign_voucher(&channel_id, 42, 1000, &secret);

        assert!(!verify_signature(&other_pubkey, &msg_hash, &sig));
    }

    #[test]
    fn test_sign_and_verify_settlement() {
        let (secret, pubkey) = make_keypair(0x01);
        let merkle_root = [0xaa; 32];
        let channel_id = [0xbb; 32];

        let msg_hash = build_settlement_message(&merkle_root, 5000, &channel_id, 1);
        let sig = sign_settlement(&msg_hash, &secret);

        assert!(verify_signature(&pubkey, &msg_hash, &sig));
    }

    #[test]
    fn test_settlement_message_deterministic() {
        let merkle_root = [0xaa; 32];
        let channel_id = [0xbb; 32];

        let h1 = build_settlement_message(&merkle_root, 100, &channel_id, 0);
        let h2 = build_settlement_message(&merkle_root, 100, &channel_id, 0);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_nonce_different_hash() {
        let merkle_root = [0xaa; 32];
        let channel_id = [0xbb; 32];

        let h1 = build_settlement_message(&merkle_root, 100, &channel_id, 0);
        let h2 = build_settlement_message(&merkle_root, 100, &channel_id, 1);
        assert_ne!(h1, h2);
    }
}
