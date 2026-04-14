use anchor_lang::prelude::*;

/// Verify an Ed25519 signature against a message and public key.
///
/// PROG-8 fix: Provides signature verification for all on-chain instructions.
///
/// Uses `ed25519_dalek` v1.x (bundled with anchor-lang 0.30 / solana-program 1.16).
/// In production, consider using Solana's ed25519_program instruction introspection
/// for parallel verification efficiency (see PROG-12).
pub fn verify_ed25519_signature(
    message: &[u8],
    signature: &[u8; 64],
    public_key: &Pubkey,
) -> bool {
    use ed25519_dalek::PublicKey;
    use ed25519_dalek::Signature;
    use ed25519_dalek::Verifier;

    let pubkey: PublicKey = match PublicKey::from_bytes(public_key.as_ref()) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let sig = match Signature::from_bytes(signature) {
        Ok(s) => s,
        Err(_) => return false,
    };

    pubkey.verify(message, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Keypair;
    use ed25519_dalek::Signer;
    use rand::rngs::OsRng;

    #[test]
    fn test_verify_valid_signature() {
        let keypair = Keypair::generate(&mut OsRng);
        let pubkey = Pubkey::new_from_array(keypair.public.to_bytes());

        let message = b"test message";
        let signature = keypair.sign(message).to_bytes();

        assert!(verify_ed25519_signature(message, &signature, &pubkey));
    }

    #[test]
    fn test_verify_wrong_message() {
        let keypair = Keypair::generate(&mut OsRng);
        let pubkey = Pubkey::new_from_array(keypair.public.to_bytes());

        let signature = keypair.sign(b"correct message").to_bytes();
        assert!(!verify_ed25519_signature(b"wrong message", &signature, &pubkey));
    }

    #[test]
    fn test_verify_wrong_pubkey() {
        let keypair = Keypair::generate(&mut OsRng);
        let other_keypair = Keypair::generate(&mut OsRng);
        let other_pubkey = Pubkey::new_from_array(other_keypair.public.to_bytes());

        let message = b"test message";
        let signature = keypair.sign(message).to_bytes();

        assert!(!verify_ed25519_signature(message, &signature, &other_pubkey));
    }

    #[test]
    fn test_verify_tampered_signature() {
        let keypair = Keypair::generate(&mut OsRng);
        let pubkey = Pubkey::new_from_array(keypair.public.to_bytes());

        let message = b"test message";
        let mut signature = keypair.sign(message).to_bytes();
        signature[0] ^= 0xff; // tamper

        assert!(!verify_ed25519_signature(message, &signature, &pubkey));
    }
}
