use hkdf::Hkdf;
use sha2::Sha256;

/// Derive an AES-256-GCM encryption key from an Ed25519 signing private key.
///
/// Uses HKDF-SHA256 with salt = `b"ignite-pay-log-v1"` and info = `user_did.as_bytes()`.
pub fn derive_log_key(signing_private: &[u8; 32], user_did: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(
        Some(b"ignite-pay-log-v1"),
        signing_private,
    );
    let mut key = [0u8; 32];
    hk.expand(user_did.as_bytes(), &mut key).expect("32 bytes");
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_same_key() {
        let privkey = [42u8; 32];
        let did = "did:example:alice";
        let k1 = derive_log_key(&privkey, did);
        let k2 = derive_log_key(&privkey, did);
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_did_different_key() {
        let privkey = [42u8; 32];
        let k1 = derive_log_key(&privkey, "did:example:alice");
        let k2 = derive_log_key(&privkey, "did:example:bob");
        assert_ne!(k1, k2);
    }

    #[test]
    fn different_privkey_different_key() {
        let did = "did:example:alice";
        let k1 = derive_log_key(&[1u8; 32], did);
        let k2 = derive_log_key(&[2u8; 32], did);
        assert_ne!(k1, k2);
    }
}
