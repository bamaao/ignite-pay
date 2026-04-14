//! Bridge layer between ignite-pay-core and ignite-pay-solana.
//! Provides DID-related on-chain operations for merchant verification.

use anyhow::Result;
use ignite_pay_solana::compression::CompressionService;
use ignite_pay_solana::indexer::IndexerClient;
use ignite_pay_solana::types::{MerchantVerification, MerkleProof};

pub use ignite_pay_solana::types::MerchantVerification as SolanaMerchantVerification;

/// Bridge for Solana DID verification operations.
pub struct SolanaDidBridge {
    compression: CompressionService,
    indexer: IndexerClient,
}

impl std::fmt::Debug for SolanaDidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaDidBridge").finish()
    }
}

impl SolanaDidBridge {
    /// Create a new SolanaDidBridge.
    pub fn new(
        rpc_url: &str,
        tree_address: &str,
        tree_authority: &str,
        das_endpoint: &str,
    ) -> Result<Self> {
        let compression = CompressionService::new(rpc_url, tree_address, tree_authority)?;
        let indexer = IndexerClient::new(das_endpoint)?;
        Ok(Self {
            compression,
            indexer,
        })
    }

    /// Verify a merchant DID is registered and trusted on-chain.
    /// Performs full Merkle proof verification.
    pub async fn verify_merchant_on_chain(
        &self,
        merchant_did: &str,
        expected_pubkey: &str,
    ) -> Result<MerchantVerification> {
        // Look up the merchant leaf via the indexer
        let tree_address = self.compression.tree_address;
        let lookup = self
            .indexer
            .find_merchant_leaf(&tree_address, merchant_did)
            .await?;

        match lookup {
            Some((leaf_index, leaf)) => {
                // Verify the active pubkey matches expected
                let expected = expected_pubkey
                    .parse::<ignite_pay_solana::solana_sdk::pubkey::Pubkey>()
                    .map_err(|e| anyhow::anyhow!("Invalid expected pubkey: {}", e))?;

                if leaf.active_pubkey != expected {
                    return Ok(MerchantVerification {
                        verified: false,
                        leaf,
                        proof: MerkleProof {
                            leaf_index,
                            proof: vec![],
                            root: [0u8; 32],
                            leaf_hash: [0u8; 32],
                        },
                    });
                }

                // Get Merkle proof from indexer
                let proof = self
                    .indexer
                    .get_merkle_proof(&tree_address, leaf_index)
                    .await?;

                // Verify locally
                let verified = self.compression.verify_proof_locally(
                    &proof.leaf_hash,
                    &proof.proof,
                    &proof.root,
                );

                Ok(MerchantVerification {
                    verified,
                    leaf,
                    proof,
                })
            }
            None => Err(anyhow::anyhow!(
                "Merchant not found on chain: {}",
                merchant_did
            )),
        }
    }

    /// Quick off-chain verification (first layer filter).
    /// Checks if the merchant DID exists in the indexer without full proof verification.
    pub async fn quick_verify(&self, merchant_did: &str) -> Result<bool> {
        let tree_address = self.compression.tree_address;
        let lookup = self
            .indexer
            .find_merchant_leaf(&tree_address, merchant_did)
            .await?;
        Ok(lookup.is_some())
    }

    /// Verify a merchant using a supplied Merkle proof (V1.1).
    /// Verifies the supplied proof_nodes against the on-chain tree root.
    pub async fn verify_merchant_with_proof(
        &self,
        merchant_did: &str,
        leaf_index: u32,
        proof_nodes: &[Vec<u8>],
    ) -> Result<bool> {
        let tree_address = self.compression.tree_address;

        // Look up the merchant leaf to get the leaf hash
        let lookup = self
            .indexer
            .find_merchant_leaf(&tree_address, merchant_did)
            .await?;

        let (found_leaf_index, leaf) = match lookup {
            Some((idx, leaf)) => (idx, leaf),
            None => return Ok(false),
        };

        // Verify the leaf index matches
        if found_leaf_index != leaf_index {
            return Ok(false);
        }

        // Get the Merkle proof from indexer to obtain the root
        let full_proof = self
            .indexer
            .get_merkle_proof(&tree_address, leaf_index)
            .await?;

        // If no proof nodes supplied, fail
        if proof_nodes.is_empty() {
            return Ok(false);
        }

        // Verify locally using the supplied proof nodes against the on-chain root
        let proof_fixed: Vec<[u8; 32]> = proof_nodes
            .iter()
            .filter_map(|node| {
                if node.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(node);
                    Some(arr)
                } else {
                    None
                }
            })
            .collect();

        if proof_fixed.len() != proof_nodes.len() {
            return Ok(false);
        }

        let verified = self.compression.verify_proof_locally(
            &full_proof.leaf_hash,
            &proof_fixed,
            &full_proof.root,
        );

        Ok(verified)
    }
}
