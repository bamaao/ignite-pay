use anyhow::Result;
use prost::Message;
use sha2::{Digest, Sha256};

use crate::audit_proto::{ChunkManifest, ChunkManifestEntry, LogChunk, TransactionEntry};
use crate::ipfs::IpfsClient;
use crate::log_chunk::{build_chunk, chunk_hash, decrypt_chunk, ChunkConfig};

/// Upload a serialized LogChunk to IPFS, returning the CID.
pub async fn upload_chunk(ipfs: &dyn IpfsClient, chunk: &LogChunk) -> Result<String> {
    let bytes = chunk.encode_to_vec();
    let cid = ipfs.upload(&bytes).await?;
    Ok(cid)
}

/// Download and deserialize a LogChunk from IPFS by CID.
pub async fn download_chunk(ipfs: &dyn IpfsClient, cid: &str) -> Result<LogChunk> {
    let bytes = ipfs.download(cid).await?;
    let chunk = LogChunk::decode(bytes.as_slice())?;
    Ok(chunk)
}

/// Upload a serialized ChunkManifest to IPFS, returning the CID.
pub async fn upload_manifest(
    ipfs: &dyn IpfsClient,
    manifest: &ChunkManifest,
) -> Result<String> {
    let bytes = manifest.encode_to_vec();
    let cid = ipfs.upload(&bytes).await?;
    Ok(cid)
}

/// Download and deserialize a ChunkManifest from IPFS by CID.
pub async fn download_manifest(
    ipfs: &dyn IpfsClient,
    cid: &str,
) -> Result<ChunkManifest> {
    let bytes = ipfs.download(cid).await?;
    let manifest = ChunkManifest::decode(bytes.as_slice())?;
    Ok(manifest)
}

/// Add an entry to the manifest for a chunk that was uploaded.
/// Computes the chunk hash and records the CID and merkle root.
pub fn add_manifest_entry(
    manifest: &mut ChunkManifest,
    chunk_id: u64,
    cid: &str,
    chunk: &LogChunk,
) {
    let hash = chunk_hash(chunk);
    let merkle_root = chunk.merkle_root.clone();

    manifest.entries.push(ChunkManifestEntry {
        chunk_id,
        cid: cid.to_string(),
        chunk_hash: hash.to_vec(),
        merkle_root,
    });
}

/// Compute SHA-256 hash of a serialized ChunkManifest (for hash chain).
pub fn manifest_hash(manifest: &ChunkManifest) -> [u8; 32] {
    let encoded = manifest.encode_to_vec();
    let mut hasher = Sha256::new();
    hasher.update(&encoded);
    hasher.finalize().into()
}

/// Full sync flow: build chunk → upload to IPFS → update manifest → upload manifest.
/// Returns (chunk_cid, new_manifest_cid).
pub async fn sync_chunk_to_ipfs(
    ipfs: &dyn IpfsClient,
    config: &ChunkConfig,
    entries: &[TransactionEntry],
    log_key: &[u8; 32],
    manifest: &mut ChunkManifest,
) -> Result<(String, String)> {
    // 1. Build encrypted chunk
    let chunk = build_chunk(config, entries, log_key)?;

    // 2. Upload chunk
    let cid = upload_chunk(ipfs, &chunk).await?;

    // 3. Update manifest
    add_manifest_entry(manifest, config.chunk_id, &cid, &chunk);

    // 4. Upload manifest
    let manifest_cid = upload_manifest(ipfs, manifest).await?;

    Ok((cid, manifest_cid))
}

/// Full restore flow: from manifest CID → download chunks → decrypt → verify hash chain.
/// Returns all restored entries sorted by nonce.
pub async fn restore_from_ipfs(
    ipfs: &dyn IpfsClient,
    manifest_cid: &str,
    log_key: &[u8; 32],
) -> Result<Vec<TransactionEntry>> {
    // 1. Download manifest
    let manifest = download_manifest(ipfs, manifest_cid).await?;

    // 2. Sort entries by chunk_id
    let mut sorted_entries = manifest.entries.clone();
    sorted_entries.sort_by_key(|e| e.chunk_id);

    // 3. Download and decrypt each chunk
    let mut all_entries: Vec<TransactionEntry> = Vec::new();
    let mut prev_hash: Option<&[u8]> = None;

    for entry in &sorted_entries {
        let chunk = download_chunk(ipfs, &entry.cid).await?;

        // Verify hash chain: chunk[i+1].prev_chunk_hash == chunk_hash(chunk[i])
        if let Some(prev) = prev_hash {
            let metadata = chunk
                .metadata
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("chunk missing metadata"))?;
            if metadata.prev_chunk_hash.as_slice() != prev {
                return Err(anyhow::anyhow!(
                    "hash chain broken at chunk {}",
                    entry.chunk_id
                ));
            }
        }

        // Verify chunk hash matches manifest
        let computed_hash = chunk_hash(&chunk);
        if computed_hash.as_slice() != entry.chunk_hash.as_slice() {
            return Err(anyhow::anyhow!(
                "chunk hash mismatch at chunk {}",
                entry.chunk_id
            ));
        }

        // Decrypt chunk
        let decrypted = decrypt_chunk(&chunk, log_key)?;
        all_entries.extend(decrypted);

        prev_hash = Some(entry.chunk_hash.as_slice());
    }

    // 4. Sort by nonce
    all_entries.sort_by_key(|e| e.nonce);

    Ok(all_entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipfs::MockIpfsClient;

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

    fn make_manifest(user_did: &str) -> ChunkManifest {
        ChunkManifest {
            user_did: user_did.to_string(),
            entries: vec![],
            prev_manifest_hash: vec![],
        }
    }

    #[tokio::test]
    async fn test_upload_download_chunk_roundtrip() {
        let ipfs = MockIpfsClient::new();
        let key = [42u8; 32];
        let config = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let entries = vec![make_entry(1, -1000, 1000)];
        let chunk = build_chunk(&config, &entries, &key).unwrap();

        let cid = upload_chunk(&ipfs, &chunk).await.unwrap();
        let downloaded = download_chunk(&ipfs, &cid).await.unwrap();

        assert_eq!(downloaded.metadata.as_ref().unwrap().chunk_id, 1);
    }

    #[tokio::test]
    async fn test_upload_download_manifest_roundtrip() {
        let ipfs = MockIpfsClient::new();
        let manifest = make_manifest("did:example:alice");

        let cid = upload_manifest(&ipfs, &manifest).await.unwrap();
        let downloaded = download_manifest(&ipfs, &cid).await.unwrap();

        assert_eq!(downloaded.user_did, "did:example:alice");
        assert!(downloaded.entries.is_empty());
    }

    #[test]
    fn test_add_manifest_entry() {
        let key = [42u8; 32];
        let config = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let entries = vec![make_entry(1, -1000, 1000)];
        let chunk = build_chunk(&config, &entries, &key).unwrap();
        let expected_hash = chunk_hash(&chunk);

        let mut manifest = make_manifest("did:example:alice");
        add_manifest_entry(&mut manifest, 1, "bafyreiTestCid1", &chunk);

        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].chunk_id, 1);
        assert_eq!(manifest.entries[0].cid, "bafyreiTestCid1");
        assert_eq!(manifest.entries[0].chunk_hash, expected_hash);
    }

    #[tokio::test]
    async fn test_sync_chunk_to_ipfs() {
        let ipfs = MockIpfsClient::new();
        let key = [42u8; 32];
        let config = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let entries = vec![
            make_entry(1, -1000, 1000),
            make_entry(2, -2000, 3000),
        ];
        let mut manifest = make_manifest("did:example:alice");

        let (chunk_cid, manifest_cid) =
            sync_chunk_to_ipfs(&ipfs, &config, &entries, &key, &mut manifest)
                .await
                .unwrap();

        assert!(!chunk_cid.is_empty());
        assert!(!manifest_cid.is_empty());
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].cid, chunk_cid);
    }

    #[tokio::test]
    async fn test_restore_from_ipfs() {
        let ipfs = MockIpfsClient::new();
        let key = [42u8; 32];

        // Build two chunks with hash chain
        let config1 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let entries1 = vec![make_entry(1, -1000, 1000)];
        let mut manifest = make_manifest("did:example:alice");
        let (cid1, _manifest_cid) =
            sync_chunk_to_ipfs(&ipfs, &config1, &entries1, &key, &mut manifest)
                .await
                .unwrap();

        let chunk1 = download_chunk(&ipfs, &cid1).await.unwrap();
        let hash1 = chunk_hash(&chunk1);

        let config2 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 2,
            prev_chunk_hash: hash1,
        };
        let entries2 = vec![make_entry(2, -2000, 3000)];
        let (_, manifest_cid) =
            sync_chunk_to_ipfs(&ipfs, &config2, &entries2, &key, &mut manifest)
                .await
                .unwrap();

        // Restore from the final manifest CID
        let restored = restore_from_ipfs(&ipfs, &manifest_cid, &key)
            .await
            .unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].nonce, 1);
        assert_eq!(restored[1].nonce, 2);
        assert_eq!(restored[0].delta_amount, -1000);
        assert_eq!(restored[1].delta_amount, -2000);
    }

    #[tokio::test]
    async fn test_restore_detects_broken_hash_chain() {
        let ipfs = MockIpfsClient::new();
        let key = [42u8; 32];

        // Build two chunks with WRONG hash chain (chunk 2 points to wrong prev)
        let config1 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 1,
            prev_chunk_hash: [0u8; 32],
        };
        let entries1 = vec![make_entry(1, -1000, 1000)];
        let mut manifest = make_manifest("did:example:alice");
        sync_chunk_to_ipfs(&ipfs, &config1, &entries1, &key, &mut manifest)
            .await
            .unwrap();

        // Chunk 2 with wrong prev_chunk_hash
        let config2 = ChunkConfig {
            user_did: "did:example:alice".to_string(),
            provider_did: "did:example:mcp".to_string(),
            chunk_id: 2,
            prev_chunk_hash: [0xFF; 32], // Wrong!
        };
        let entries2 = vec![make_entry(2, -2000, 3000)];
        let (_, manifest_cid) =
            sync_chunk_to_ipfs(&ipfs, &config2, &entries2, &key, &mut manifest)
                .await
                .unwrap();

        let result = restore_from_ipfs(&ipfs, &manifest_cid, &key).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_hash_deterministic() {
        let manifest = make_manifest("did:example:alice");
        let h1 = manifest_hash(&manifest);
        let h2 = manifest_hash(&manifest);
        assert_eq!(h1, h2);
    }
}
