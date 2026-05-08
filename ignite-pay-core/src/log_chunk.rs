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

use anyhow::{anyhow, Result};
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, AeadCore, Nonce};
use prost::Message;
use sha2::{Digest, Sha256};

use crate::audit_merkle::AuditMerkleTree;
use crate::audit_proto::{
    ChunkMetadata, EncryptedPayload, LogChunk, TransactionEntry,
};

/// Configuration for building a LogChunk.
pub struct ChunkConfig {
    pub user_did: String,
    pub provider_did: String,
    pub chunk_id: u64,
    pub prev_chunk_hash: [u8; 32],
}

/// Build an encrypted LogChunk from a list of transaction entries.
///
/// Pipeline: entries → Merkle root → serialize → Zstd compress → AES-256-GCM encrypt → LogChunk
pub fn build_chunk(
    config: &ChunkConfig,
    entries: &[TransactionEntry],
    log_key: &[u8; 32],
) -> Result<LogChunk> {
    if entries.is_empty() {
        return Err(anyhow!("cannot build chunk with zero entries"));
    }

    // 1. Compute leaf hashes: sha256(protobuf_encoded) for each entry
    let leaves: Vec<Vec<u8>> = entries
        .iter()
        .map(|e| {
            let mut buf = Vec::new();
            e.encode(&mut buf)
                .expect("protobuf encode should not fail");
            buf
        })
        .collect();

    // 2. Build Merkle tree
    let tree = AuditMerkleTree::new(&leaves);
    let merkle_root = tree.root().to_vec();

    // 3. Serialize EncryptedPayload
    let payload = EncryptedPayload {
        entries: entries.to_vec(),
    };
    let plaintext = payload.encode_to_vec();

    // 4. Zstd compress (level 3)
    let compressed = zstd::encode_all(plaintext.as_slice(), 3)
        .map_err(|e| anyhow!("zstd compression failed: {e}"))?;

    // 5. AES-256-GCM encrypt
    let cipher = Aes256Gcm::new_from_slice(log_key)
        .map_err(|e| anyhow!("invalid AES key: {e}"))?;
    let iv = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bit
    let aad = config.chunk_id.to_be_bytes();
    let ciphertext = cipher
        .encrypt(&iv, aes_gcm::aead::Payload {
            msg: &compressed,
            aad: &aad,
        })
        .map_err(|e| anyhow!("AES-GCM encryption failed: {e}"))?;

    // aes-gcm appends the 16-byte tag to the ciphertext
    let (ciphertext_core, auth_tag) = ciphertext.split_at(ciphertext.len() - 16);

    // Prepend IV (12 bytes) to encrypted_payload so decrypt_chunk can recover it
    let mut encrypted_payload = Vec::with_capacity(12 + ciphertext_core.len());
    encrypted_payload.extend_from_slice(&iv);
    encrypted_payload.extend_from_slice(ciphertext_core);

    // 6. Extract nonces and timestamps for metadata
    let start_nonce = entries.first().unwrap().nonce;
    let end_nonce = entries.last().unwrap().nonce;
    let timestamp_start = entries.iter().map(|e| e.timestamp).min().unwrap_or(0);
    let timestamp_end = entries.iter().map(|e| e.timestamp).max().unwrap_or(0);

    // 7. Assemble LogChunk
    let metadata = ChunkMetadata {
        user_did: config.user_did.clone(),
        provider_did: config.provider_did.clone(),
        chunk_id: config.chunk_id,
        start_nonce,
        end_nonce,
        prev_chunk_hash: config.prev_chunk_hash.to_vec(),
        timestamp_start,
        timestamp_end,
    };

    Ok(LogChunk {
        metadata: Some(metadata),
        encrypted_payload: encrypted_payload.to_vec(),
        auth_tag: auth_tag.to_vec(),
        merkle_root,
    })
}

/// Decrypt and verify a LogChunk, returning the inner transaction entries.
///
/// Reverse pipeline: extract IV → decrypt → decompress → deserialize → verify Merkle root
pub fn decrypt_chunk(chunk: &LogChunk, log_key: &[u8; 32]) -> Result<Vec<TransactionEntry>> {
    let metadata = chunk
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("chunk missing metadata"))?;

    if chunk.encrypted_payload.len() < 12 {
        return Err(anyhow!("encrypted_payload too short (missing IV)"));
    }

    // 1. Extract IV (first 12 bytes) and ciphertext
    let (iv_bytes, ciphertext_core) = chunk.encrypted_payload.split_at(12);
    let iv = Nonce::from_slice(iv_bytes);

    // 2. Reconstruct ciphertext + tag for aes-gcm
    let mut ciphertext = ciphertext_core.to_vec();
    ciphertext.extend_from_slice(&chunk.auth_tag);

    // 3. AES-256-GCM decrypt
    let cipher = Aes256Gcm::new_from_slice(log_key)
        .map_err(|e| anyhow!("invalid AES key: {e}"))?;
    let aad = metadata.chunk_id.to_be_bytes();
    let compressed = cipher
        .decrypt(&iv, aes_gcm::aead::Payload {
            msg: &ciphertext,
            aad: &aad,
        })
        .map_err(|e| anyhow!("AES-GCM decryption failed: {e}"))?;

    // 4. Zstd decompress
    let plaintext = zstd::decode_all(compressed.as_slice())
        .map_err(|e| anyhow!("zstd decompression failed: {e}"))?;

    // 5. Deserialize
    let payload = EncryptedPayload::decode(plaintext.as_slice())
        .map_err(|e| anyhow!("protobuf decode failed: {e}"))?;

    // 6. Verify Merkle root
    let leaves: Vec<Vec<u8>> = payload
        .entries
        .iter()
        .map(|e| {
            let mut buf = Vec::new();
            e.encode(&mut buf)
                .expect("protobuf encode should not fail");
            buf
        })
        .collect();
    let tree = AuditMerkleTree::new(&leaves);
    let computed_root = tree.root();
    if computed_root.as_slice() != chunk.merkle_root {
        return Err(anyhow!("merkle root mismatch"));
    }

    Ok(payload.entries)
}

/// Compute SHA-256 hash of a serialized LogChunk (for hash chain).
pub fn chunk_hash(chunk: &LogChunk) -> [u8; 32] {
    let encoded = chunk.encode_to_vec();
    let mut hasher = Sha256::new();
    hasher.update(&encoded);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(nonce: u64, delta: i64, cumulative: u64) -> TransactionEntry {
        TransactionEntry {
            nonce,
            delta_amount: delta,
            cumulative_amount: cumulative,
            signature: vec![],
            timestamp: 1700000000 + nonce as i64,
            service_id: format!("/api/v1/call/{nonce}"),
            payment_id: format!("pay-{nonce}"),
            merchant_did: "did:example:merchant".to_string(),
            memo: vec![],
        }
    }

    fn test_config() -> ChunkConfig {
        ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        }
    }

    #[test]
    fn build_requires_entries() {
        let config = test_config();
        let key = [42u8; 32];
        let result = build_chunk(&config, &[], &key);
        assert!(result.is_err());
    }

    #[test]
    fn build_single_entry() {
        let config = test_config();
        let key = [42u8; 32];
        let entries = vec![make_entry(1, -1000, 1000)];

        let chunk = build_chunk(&config, &entries, &key).unwrap();

        let meta = chunk.metadata.as_ref().unwrap();
        assert_eq!(meta.user_did, "did:example:alice");
        assert_eq!(meta.provider_did, "did:example:mcp");
        assert_eq!(meta.chunk_id, 1);
        assert_eq!(meta.start_nonce, 1);
        assert_eq!(meta.end_nonce, 1);
        assert_eq!(meta.prev_chunk_hash, vec![0u8; 32]);
        assert!(!chunk.encrypted_payload.is_empty());
        assert_eq!(chunk.auth_tag.len(), 16);
        assert_eq!(chunk.merkle_root.len(), 32);
    }

    #[test]
    fn build_multiple_entries() {
        let config = test_config();
        let key = [42u8; 32];
        let entries = vec![
            make_entry(1, -1000, 1000),
            make_entry(2, -2000, 3000),
            make_entry(3, -500, 3500),
        ];

        let chunk = build_chunk(&config, &entries, &key).unwrap();

        let meta = chunk.metadata.as_ref().unwrap();
        assert_eq!(meta.start_nonce, 1);
        assert_eq!(meta.end_nonce, 3);
        assert!(meta.timestamp_start <= meta.timestamp_end);
    }

    #[test]
    fn merkle_root_consistency() {
        let config = test_config();
        let key = [42u8; 32];
        let entries = vec![
            make_entry(1, -1000, 1000),
            make_entry(2, -2000, 3000),
        ];

        // Build twice, same merkle root
        let chunk1 = build_chunk(&config, &entries, &key).unwrap();
        let chunk2 = build_chunk(&config, &entries, &key).unwrap();
        assert_eq!(chunk1.merkle_root, chunk2.merkle_root);
    }

    #[test]
    fn hash_chain_prev_chunk() {
        let key = [42u8; 32];
        let entries1 = vec![make_entry(1, -1000, 1000)];
        let entries2 = vec![make_entry(2, -2000, 3000)];

        let config1 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let chunk1 = build_chunk(&config1, &entries1, &key).unwrap();
        let hash1 = chunk_hash(&chunk1);

        let config2 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 2,
            prev_chunk_hash: hash1,
        };
        let chunk2 = build_chunk(&config2, &entries2, &key).unwrap();

        let meta2 = chunk2.metadata.as_ref().unwrap();
        assert_eq!(meta2.prev_chunk_hash, hash1.to_vec());
    }

    #[test]
    fn different_key_different_ciphertext() {
        let config = test_config();
        let entries = vec![make_entry(1, -1000, 1000)];

        let chunk_a = build_chunk(&config, &entries, &[1u8; 32]).unwrap();
        let chunk_b = build_chunk(&config, &entries, &[2u8; 32]).unwrap();

        // Same merkle root (from same entries)
        assert_eq!(chunk_a.merkle_root, chunk_b.merkle_root);
        // Different ciphertext
        assert_ne!(chunk_a.encrypted_payload, chunk_b.encrypted_payload);
    }

    #[test]
    fn build_decrypt_roundtrip() {
        let config = test_config();
        let key = [42u8; 32];
        let original_entries = vec![
            make_entry(1, -1000, 1000),
            make_entry(2, -2000, 3000),
            make_entry(3, -500, 3500),
        ];

        let chunk = build_chunk(&config, &original_entries, &key).unwrap();
        let decrypted = decrypt_chunk(&chunk, &key).unwrap();

        assert_eq!(decrypted.len(), original_entries.len());
        for (got, want) in decrypted.iter().zip(original_entries.iter()) {
            assert_eq!(got.nonce, want.nonce);
            assert_eq!(got.delta_amount, want.delta_amount);
            assert_eq!(got.cumulative_amount, want.cumulative_amount);
            assert_eq!(got.timestamp, want.timestamp);
            assert_eq!(got.service_id, want.service_id);
            assert_eq!(got.payment_id, want.payment_id);
            assert_eq!(got.merchant_did, want.merchant_did);
        }
    }

    #[test]
    fn wrong_key_fails() {
        let config = test_config();
        let entries = vec![make_entry(1, -1000, 1000)];

        let chunk = build_chunk(&config, &entries, &[42u8; 32]).unwrap();
        let result = decrypt_chunk(&chunk, &[99u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn tampered_merkle_root_fails() {
        let config = test_config();
        let key = [42u8; 32];
        let entries = vec![make_entry(1, -1000, 1000)];

        let mut chunk = build_chunk(&config, &entries, &key).unwrap();
        // Tamper with merkle root
        chunk.merkle_root[0] ^= 0xFF;
        let result = decrypt_chunk(&chunk, &key);
        assert!(result.is_err());
    }
}
