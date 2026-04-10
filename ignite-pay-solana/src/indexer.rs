use crate::error::{Result, SolanaError};
use crate::types::{MerchantLeaf, MerkleProof};
use solana_sdk::pubkey::Pubkey;

/// Client for querying compressed account data via Helius DAS API.
pub struct IndexerClient {
    http_client: reqwest::Client,
    das_endpoint: String,
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
        })
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
        let root = bs58::decode(root_str).into_vec().map_err(|e| {
            SolanaError::IndexerError(format!("Failed to decode root: {}", e))
        })?;
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
        let leaf_hash_vec = bs58::decode(leaf_hash_str).into_vec().map_err(|e| {
            SolanaError::IndexerError(format!("Failed to decode leaf hash: {}", e))
        })?;
        let mut leaf_hash = [0u8; 32];
        leaf_hash.copy_from_slice(&leaf_hash_vec[..32]);

        Ok(MerkleProof {
            leaf_index,
            proof,
            root: root_bytes,
            leaf_hash,
        })
    }

    /// Find a merchant leaf by DID.
    /// In production, this queries the Helius indexer for the matching compressed asset.
    /// For now, returns None to indicate the lookup should be done externally.
    pub async fn find_merchant_leaf(
        &self,
        tree_address: &Pubkey,
        merchant_did: &str,
    ) -> Result<Option<(u32, MerchantLeaf)>> {
        // In production, this would use Helius DAS search API to find the leaf.
        // The actual implementation depends on the indexing service setup.
        // For V2.0, the leaf index is stored alongside the merchant DID in the off-chain database.
        tracing::warn!(
            "find_merchant_leaf: lookup for {} in tree {} - requires external index",
            merchant_did,
            tree_address
        );
        Ok(None)
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

        let root_vec = bs58::decode(root_str).into_vec().map_err(|e| {
            SolanaError::IndexerError(format!("Failed to decode root: {}", e))
        })?;

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
}
