/// Extract the Ed25519 public key bytes from a did:ignite identifier.
/// Format: did:ignite:z + Base58(0xed 0x01 + Ed25519_pubkey)
///
/// Mirrors `ignite-pay-core/src/identity.rs:167-184`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_valid_did() {
        // Manually construct a valid DID: 0xed 0x01 + 32 zero bytes
        let mut payload = vec![0xed, 0x01];
        payload.extend_from_slice(&[0u8; 32]);
        let encoded = bs58::encode(&payload).into_string();
        let did = format!("did:ignite:z{}", encoded);

        let result = extract_pubkey_from_did(&did);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), [0u8; 32]);
    }

    #[test]
    fn test_extract_invalid_prefix() {
        assert!(extract_pubkey_from_did("did:key:zSomething").is_none());
    }

    #[test]
    fn test_extract_invalid_multicodec() {
        // 0xed 0x02 is wrong codec
        let mut payload = vec![0xed, 0x02];
        payload.extend_from_slice(&[0u8; 32]);
        let encoded = bs58::encode(&payload).into_string();
        let did = format!("did:ignite:z{}", encoded);
        assert!(extract_pubkey_from_did(&did).is_none());
    }

    #[test]
    fn test_extract_short_payload() {
        let payload = vec![0xed, 0x01, 0x00];
        let encoded = bs58::encode(&payload).into_string();
        let did = format!("did:ignite:z{}", encoded);
        assert!(extract_pubkey_from_did(&did).is_none());
    }

    #[test]
    fn test_extract_invalid_base58() {
        let did = "did:ignite:z!!!invalid!!!";
        assert!(extract_pubkey_from_did(did).is_none());
    }
}
