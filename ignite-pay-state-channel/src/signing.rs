use crate::types::{LeafUpdate, UTXOLeaf};
use ed25519_dalek::{Keypair, PublicKey, Signature, Signer as EdSigner};
use solana_program::hash::hash;
use solana_pubkey::Pubkey;

/// Construct the message bytes for a leaf update signature.
///
/// Format: SHA-256(channel_id || sequence || leaf_index || prev_leaf_hash || new_leaf_hash)
pub fn leaf_update_message(
    channel_id: &[u8; 32],
    sequence: u64,
    leaf_index: u32,
    prev_leaf_hash: &[u8; 32],
    new_leaf_hash: &[u8; 32],
) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 + 8 + 4 + 32 + 32);
    data.extend_from_slice(channel_id);
    data.extend_from_slice(&sequence.to_le_bytes());
    data.extend_from_slice(&leaf_index.to_le_bytes());
    data.extend_from_slice(prev_leaf_hash);
    data.extend_from_slice(new_leaf_hash);
    hash(&data).to_bytes()
}

/// Construct the message bytes for a claim-specific signature.
///
/// BUG-34 fix: Uses a dedicated message format for Claim/VerifyHTLC/HTLCRefund
/// to prevent replay within the same slot. Includes a domain separator ("claim"),
/// channel_id, leaf_index, amount, and current_slot.
///
/// Format: SHA-256("claim" || channel_id || leaf_index || amount || current_slot)
pub fn claim_message(
    channel_id: &[u8; 32],
    leaf_index: u32,
    amount: u64,
    current_slot: u64,
) -> [u8; 32] {
    let mut data = Vec::with_capacity(5 + 32 + 4 + 8 + 8);
    data.extend_from_slice(b"claim");
    data.extend_from_slice(channel_id);
    data.extend_from_slice(&leaf_index.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&current_slot.to_le_bytes());
    hash(&data).to_bytes()
}

/// Construct the message bytes for a state (root) signature.
///
/// Format: SHA-256(channel_id || sequence || root)
pub fn state_message(
    channel_id: &[u8; 32],
    sequence: u64,
    root: &[u8; 32],
) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 + 8 + 32);
    data.extend_from_slice(channel_id);
    data.extend_from_slice(&sequence.to_le_bytes());
    data.extend_from_slice(root);
    hash(&data).to_bytes()
}

/// Sign a leaf update and return the complete LeafUpdate struct.
pub fn sign_leaf_update(
    channel_id: &[u8; 32],
    sequence: u64,
    leaf_index: u32,
    prev_leaf: &UTXOLeaf,
    new_leaf: UTXOLeaf,
    signer: &Keypair,
) -> LeafUpdate {
    let prev_hash = prev_leaf.hash();
    let new_hash = new_leaf.hash();
    let message = leaf_update_message(channel_id, sequence, leaf_index, &prev_hash, &new_hash);
    let ed_signature: Signature = signer.sign(&message);

    LeafUpdate {
        channel_id: *channel_id,
        sequence,
        leaf_index,
        prev_leaf_hash: prev_hash,
        new_leaf,
        signature: ed_signature.to_bytes(),
    }
}

/// Verify the signature on a leaf update.
pub fn verify_leaf_update_signature(update: &LeafUpdate, pubkey: &Pubkey) -> bool {
    let new_hash = update.new_leaf.hash();
    let message = leaf_update_message(
        &update.channel_id,
        update.sequence,
        update.leaf_index,
        &update.prev_leaf_hash,
        &new_hash,
    );
    let ed_pubkey = match PublicKey::from_bytes(pubkey.as_ref()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let ed_sig = match Signature::from_bytes(&update.signature) {
        Ok(s) => s,
        Err(_) => return false,
    };
    ed_pubkey.verify_strict(&message, &ed_sig).is_ok()
}

/// Sign a state (channel root) and return the fixed-size signature.
pub fn sign_state(
    channel_id: &[u8; 32],
    sequence: u64,
    root: &[u8; 32],
    keypair: &Keypair,
) -> [u8; 64] {
    let message = state_message(channel_id, sequence, root);
    let ed_sig: Signature = keypair.sign(&message);
    ed_sig.to_bytes()
}

/// Verify a state (channel root) signature.
pub fn verify_state_signature(
    channel_id: &[u8; 32],
    sequence: u64,
    root: &[u8; 32],
    sig: &[u8; 64],
    pubkey: &Pubkey,
) -> bool {
    let message = state_message(channel_id, sequence, root);
    verify_ed25519_signature(&message, sig, pubkey)
}

/// Verify an Ed25519 signature against an arbitrary message hash.
pub fn verify_ed25519_signature(
    message: &[u8; 32],
    sig: &[u8; 64],
    pubkey: &Pubkey,
) -> bool {
    let ed_pubkey = match PublicKey::from_bytes(pubkey.as_ref()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let ed_sig = match Signature::from_bytes(sig) {
        Ok(s) => s,
        Err(_) => return false,
    };
    ed_pubkey.verify_strict(message, &ed_sig).is_ok()
}

/// Generate a new Ed25519 keypair.
pub fn generate_keypair() -> Keypair {
    let mut csprng = rand_core::OsRng;
    Keypair::generate(&mut csprng)
}

/// Extract a Solana Pubkey from an ed25519_dalek Keypair.
pub fn to_pubkey(kp: &Keypair) -> Pubkey {
    Pubkey::new_from_array(kp.public.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keypair() -> Keypair {
        generate_keypair()
    }

    #[test]
    fn test_leaf_update_sign_verify_roundtrip() {
        let signer = make_keypair();
        let channel_id = [42u8; 32];
        let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 100);
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 50);

        let update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
        assert!(verify_leaf_update_signature(&update, &to_pubkey(&signer)));
    }

    #[test]
    fn test_leaf_update_tampered_data_rejected() {
        let signer = make_keypair();
        let channel_id = [42u8; 32];
        let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 100);
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 50);

        let mut update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
        update.new_leaf.amount = 999;
        assert!(!verify_leaf_update_signature(&update, &to_pubkey(&signer)));
    }

    #[test]
    fn test_leaf_update_wrong_pubkey_rejected() {
        let signer = make_keypair();
        let wrong_key = make_keypair();
        let channel_id = [42u8; 32];
        let prev_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 100);
        let new_leaf = UTXOLeaf::standard(Pubkey::new_unique(), 50);

        let update = sign_leaf_update(&channel_id, 1, 0, &prev_leaf, new_leaf, &signer);
        assert!(!verify_leaf_update_signature(&update, &to_pubkey(&wrong_key)));
    }

    #[test]
    fn test_state_sign_verify_roundtrip() {
        let keypair = make_keypair();
        let channel_id = [7u8; 32];
        let root = [99u8; 32];

        let sig = sign_state(&channel_id, 5, &root, &keypair);
        assert!(verify_state_signature(&channel_id, 5, &root, &sig, &to_pubkey(&keypair)));
    }

    #[test]
    fn test_state_wrong_sequence_rejected() {
        let keypair = make_keypair();
        let channel_id = [7u8; 32];
        let root = [99u8; 32];

        let sig = sign_state(&channel_id, 5, &root, &keypair);
        assert!(!verify_state_signature(&channel_id, 6, &root, &sig, &to_pubkey(&keypair)));
    }

    #[test]
    fn test_state_wrong_root_rejected() {
        let keypair = make_keypair();
        let channel_id = [7u8; 32];
        let root = [99u8; 32];
        let wrong_root = [0u8; 32];

        let sig = sign_state(&channel_id, 5, &root, &keypair);
        assert!(!verify_state_signature(&channel_id, 5, &wrong_root, &sig, &to_pubkey(&keypair)));
    }

    #[test]
    fn test_messages_deterministic() {
        let channel_id = [1u8; 32];
        let m1 = state_message(&channel_id, 1, &[2u8; 32]);
        let m2 = state_message(&channel_id, 1, &[2u8; 32]);
        assert_eq!(m1, m2);

        let m3 = state_message(&channel_id, 2, &[2u8; 32]);
        assert_ne!(m1, m3, "Different sequence should produce different message");
    }
}
