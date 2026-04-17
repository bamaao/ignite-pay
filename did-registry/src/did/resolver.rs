use sha2::{Digest, Sha256};

/// Compute a deterministic hash for a DID identifier.
/// Returns SHA-256(did_string).
pub fn compute_did_hash(did: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(did.as_bytes());
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Verify an Ed25519 signature from a did:ignite DID key.
/// Extracts the public key from the DID and verifies the signature.
pub fn verify_did_signature(did: &str, message: &str, signature_b64: &str) -> bool {
    // Extract public key from did:ignite
    let pk_bytes = match extract_pubkey_from_did(did) {
        Some(bytes) => bytes,
        None => return false,
    };

    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes) {
        Ok(key) => key,
        Err(_) => return false,
    };

    let signature_bytes = match base64_decode(signature_b64) {
        Some(bytes) => bytes,
        None => return false,
    };

    if signature_bytes.len() != 64 {
        return false;
    }

    let sig = match ed25519_dalek::Signature::try_from(signature_bytes.as_slice()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    use ed25519_dalek::Verifier;
    verifying_key.verify(message.as_bytes(), &sig).is_ok()
}

/// Extract the Ed25519 public key bytes from a did:ignite identifier.
/// Format: did:ignite:z + Base58(0xed 0x01 + Ed25519_pubkey)
pub fn extract_pubkey_from_did(did: &str) -> Option<[u8; 32]> {
    let prefix = "did:ignite:z";
    if !did.starts_with(prefix) {
        return None;
    }

    let encoded = &did[prefix.len()..];
    let decoded = bs58::decode(encoded).into_vec().ok()?;

    // Expect multicodec prefix 0xed 0x01 + 32 bytes Ed25519 pubkey
    if decoded.len() != 34 || decoded[0] != 0xed || decoded[1] != 0x01 {
        return None;
    }

    let mut pk = [0u8; 32];
    pk.copy_from_slice(&decoded[2..34]);
    Some(pk)
}

/// Simple base64 decode without external crate.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0;

    for ch in input.chars() {
        let val = TABLE.iter().position(|&b| b as char == ch)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
        }
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_did_hash() {
        let hash1 = compute_did_hash("did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK");
        let hash2 = compute_did_hash("did:ignite:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK");
        assert_eq!(hash1, hash2, "Same DID should produce same hash");

        let hash3 = compute_did_hash("did:ignite:z6Mkother...");
        assert_ne!(hash1, hash3, "Different DIDs should produce different hashes");
    }

    #[test]
    fn test_extract_pubkey_from_did_invalid() {
        assert!(extract_pubkey_from_did("did:key:abc").is_none());
        assert!(extract_pubkey_from_did("did:ignite:abc").is_none()); // no 'z' prefix
    }

    #[test]
    fn test_base64_decode() {
        let decoded = base64_decode("SGVsbG8=").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello");
    }
}
