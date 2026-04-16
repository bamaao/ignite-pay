use crate::error::{Result, SolanaError};
use crate::types::{MerchantLeaf, MerkleProof};
use solana_sdk::pubkey::Pubkey;

/// Client for querying compressed account data via Helius DAS API.
pub struct IndexerClient {
    http_client: reqwest::Client,
    das_endpoint: String,
    /// Optional sled tree for local DID → leaf_index mapping
    did_index: Option<sled::Tree>,
}

impl std::fmt::Debug for IndexerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexerClient")
            .field("das_endpoint", &self.das_endpoint)
            .finish()
    }
}

impl IndexerClient {
    /// Create a new IndexerClient pointing at a Helius DAS API endpoint.
    pub fn new(das_endpoint: &str) -> Result<Self> {
        Ok(Self {
            http_client: reqwest::Client::new(),
            das_endpoint: das_endpoint.to_string(),
            did_index: None,
        })
    }

    /// Create an IndexerClient with a sled-backed DID index for persistent lookups.
    pub fn with_index(das_endpoint: &str, db: &sled::Db) -> Result<Self> {
        let tree = db
            .open_tree("did_index")
            .map_err(|e| SolanaError::SledError(e))?;
        Ok(Self {
            http_client: reqwest::Client::new(),
            das_endpoint: das_endpoint.to_string(),
            did_index: Some(tree),
        })
    }

    /// SHA-256 hash of a merchant DID string.
    pub fn hash_merchant_did(did: &str) -> [u8; 32] {
        use solana_sdk::hash::hash;
        hash(did.as_bytes()).to_bytes()
    }

    /// Register a merchant DID → leaf_index mapping in the local index.
    /// Also stores the serialized MerchantLeaf for direct retrieval.
    pub fn register_merchant_index(
        &self,
        merchant_did: &str,
        leaf_index: u32,
        leaf: &MerchantLeaf,
    ) -> Result<()> {
        let index = self
            .did_index
            .as_ref()
            .ok_or_else(|| SolanaError::IndexerError("No DID index configured".into()))?;

        let did_hash = Self::hash_merchant_did(merchant_did);
        // Key: DID hash (32 bytes), Value: leaf_index (4 bytes LE) + borsh(MerchantLeaf)
        let mut value = leaf_index.to_le_bytes().to_vec();
        value.extend_from_slice(&borsh::to_vec(leaf)?);
        index.insert(&did_hash, value)?;
        index.flush()?;
        tracing::info!(
            "Registered merchant DID index: {} -> leaf {}",
            merchant_did,
            leaf_index
        );
        Ok(())
    }

    /// Find a merchant leaf by DID using the local index + DAS API proof.
    /// Returns (leaf_index, MerchantLeaf) if found.
    pub async fn find_merchant_leaf(
        &self,
        tree_address: &Pubkey,
        merchant_did: &str,
    ) -> Result<Option<(u32, MerchantLeaf)>> {
        let index = match self.did_index.as_ref() {
            Some(idx) => idx,
            None => {
                tracing::warn!(
                    "find_merchant_leaf: no DID index configured, cannot look up {}",
                    merchant_did
                );
                return Ok(None);
            }
        };

        let did_hash = Self::hash_merchant_did(merchant_did);
        match index.get(&did_hash)? {
            Some(value) => {
                if value.len() < 4 {
                    return Err(SolanaError::IndexerError(
                        "Invalid index entry: too short".into(),
                    ));
                }
                let leaf_index = u32::from_le_bytes(
                    value[..4].try_into().map_err(|_| {
                        SolanaError::IndexerError("Failed to parse leaf_index".into())
                    })?,
                );
                let leaf: MerchantLeaf = borsh::from_slice(&value[4..])?;
                tracing::info!(
                    "Found merchant {} at leaf index {} via local index",
                    merchant_did,
                    leaf_index
                );
                Ok(Some((leaf_index, leaf)))
            }
            None => {
                tracing::info!(
                    "Merchant {} not found in local DID index for tree {}",
                    merchant_did,
                    tree_address
                );
                Ok(None)
            }
        }
    }

    /// Fetch a Merkle proof for a compressed leaf by tree address and leaf index.
    pub async fn get_merkle_proof(
        &self,
        tree_address: &Pubkey,
        leaf_index: u32,
    ) -> Result<MerkleProof> {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAssetProof",
            "params": {
                "id": tree_address.to_string(),
                "leaf_index": leaf_index,
            }
        });

        let response = self
            .http_client
            .post(&self.das_endpoint)
            .json(&request_body)
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;

        let result = json
            .get("result")
            .ok_or_else(|| SolanaError::IndexerError("No result in DAS response".into()))?;

        let root_str = result
            .get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SolanaError::IndexerError("No root in proof response".into()))?;
        let root = bs58::decode(root_str)
            .into_vec()
            .map_err(|e| SolanaError::IndexerError(format!("Failed to decode root: {}", e)))?;
        let mut root_bytes = [0u8; 32];
        root_bytes.copy_from_slice(&root[..32]);

        let proof_array = result
            .get("proof")
            .and_then(|v| v.as_array())
            .ok_or_else(|| SolanaError::IndexerError("No proof array in response".into()))?;

        let mut proof: Vec<[u8; 32]> = Vec::with_capacity(proof_array.len());
        for p in proof_array {
            let p_str = p
                .as_str()
                .ok_or_else(|| SolanaError::IndexerError("Proof element not a string".into()))?;
            let p_bytes = bs58::decode(p_str).into_vec().map_err(|e| {
                SolanaError::IndexerError(format!("Failed to decode proof element: {}", e))
            })?;
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&p_bytes[..32]);
            proof.push(arr);
        }

        let leaf_hash_str = result
            .get("hash")
            .or_else(|| result.get("leaf"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| SolanaError::IndexerError("No leaf hash in response".into()))?;
        let leaf_hash_vec = bs58::decode(leaf_hash_str)
            .into_vec()
            .map_err(|e| SolanaError::IndexerError(format!("Failed to decode leaf hash: {}", e)))?;
        let mut leaf_hash = [0u8; 32];
        leaf_hash.copy_from_slice(&leaf_hash_vec[..32]);

        Ok(MerkleProof {
            leaf_index,
            proof,
            root: root_bytes,
            leaf_hash,
        })
    }

    /// Get the current root hash of the Concurrent Merkle Tree.
    pub async fn get_tree_root(&self, tree_address: &Pubkey) -> Result<[u8; 32]> {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAssetProof",
            "params": {
                "id": tree_address.to_string(),
            }
        });

        let response = self
            .http_client
            .post(&self.das_endpoint)
            .json(&request_body)
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;

        let result = json
            .get("result")
            .ok_or_else(|| SolanaError::IndexerError("No result in DAS response".into()))?;

        let root_str = result
            .get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SolanaError::IndexerError("No root in response".into()))?;

        let root_vec = bs58::decode(root_str)
            .into_vec()
            .map_err(|e| SolanaError::IndexerError(format!("Failed to decode root: {}", e)))?;

        let mut root = [0u8; 32];
        root.copy_from_slice(&root_vec[..32]);
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexer_client_new() {
        let client = IndexerClient::new("https://mainnet.helius-rpc.com/?api-key=test");
        assert!(client.is_ok());
    }

    #[test]
    fn test_indexer_with_did_index() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let client = IndexerClient::with_index(
            "https://mainnet.helius-rpc.com/?api-key=test",
            &db,
        )
        .unwrap();
        assert!(client.did_index.is_some());
    }

    #[test]
    fn test_register_and_find_merchant() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let client =
            IndexerClient::with_index("https://mainnet.helius-rpc.com/?api-key=test", &db)
                .unwrap();

        let leaf = MerchantLeaf {
            merchant_did_hash: IndexerClient::hash_merchant_did("did:ignite:zTestMerchant"),
            active_pubkey: Pubkey::new_unique(),
            platform_vc_hash: [0u8; 32],
            status: 0,
            slot_updated: 100,
        };

        client
            .register_merchant_index("did:ignite:zTestMerchant", 5, &leaf)
            .unwrap();

        // Synchronous lookup in local index
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(client.find_merchant_leaf(
                &Pubkey::new_unique(),
                "did:ignite:zTestMerchant",
            ))
            .unwrap();

        assert!(result.is_some());
        let (index, found_leaf) = result.unwrap();
        assert_eq!(index, 5);
        assert_eq!(found_leaf.merchant_did_hash, leaf.merchant_did_hash);
        assert_eq!(found_leaf.active_pubkey, leaf.active_pubkey);
    }

    #[test]
    fn test_find_merchant_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let client =
            IndexerClient::with_index("https://mainnet.helius-rpc.com/?api-key=test", &db)
                .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(client.find_merchant_leaf(
                &Pubkey::new_unique(),
                "did:ignite:zNonexistent",
            ))
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_merchant_no_index() {
        let client = IndexerClient::new("https://mainnet.helius-rpc.com/?api-key=test").unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(client.find_merchant_leaf(
                &Pubkey::new_unique(),
                "did:ignite:zTestMerchant",
            ))
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_hash_merchant_did_deterministic() {
        let h1 = IndexerClient::hash_merchant_did("did:ignite:zTest");
        let h2 = IndexerClient::hash_merchant_did("did:ignite:zTest");
        assert_eq!(h1, h2);

        let h3 = IndexerClient::hash_merchant_did("did:ignite:zOther");
        assert_ne!(h1, h3);
    }
}
