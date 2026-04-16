use crate::error::{Result, SolanaError};
use crate::indexer::IndexerClient;
use crate::types::MerchantLeaf;
use solana_client::rpc_client::RpcClient;
use solana_sdk::hash::{hash, hashv};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;

/// Compute the Anchor instruction discriminator: sha256("global:<name>")[..8]
fn anchor_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let h = hash(preimage.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&h.to_bytes()[..8]);
    disc
}

/// Service for interacting with SPL Account Compression (Concurrent Merkle Tree).
pub struct CompressionService {
    pub rpc_client: RpcClient,
    pub tree_address: Pubkey,
    pub tree_authority: Pubkey,
}

impl std::fmt::Debug for CompressionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressionService")
            .field("tree_address", &self.tree_address)
            .field("tree_authority", &self.tree_authority)
            .finish()
    }
}

impl CompressionService {
    /// Create a new CompressionService.
    pub fn new(rpc_url: &str, tree_address: &str, authority: &str) -> Result<Self> {
        let tree_address = tree_address
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;
        let tree_authority = authority
            .parse::<Pubkey>()
            .map_err(|e| SolanaError::InvalidPubkey(e.to_string()))?;
        Ok(Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            tree_address,
            tree_authority,
        })
    }

    /// Compute the leaf hash for a merchant leaf node.
    /// Uses solana_sdk::hash::hashv to deterministically combine fields.
    pub fn compute_leaf_hash(leaf: &MerchantLeaf) -> [u8; 32] {
        let active_pubkey_bytes = leaf.active_pubkey.to_bytes();
        let slot_bytes = leaf.slot_updated.to_le_bytes();
        let status_bytes = [leaf.status];
        hashv(&[
            &leaf.merchant_did_hash,
            &active_pubkey_bytes,
            &leaf.platform_vc_hash,
            &status_bytes,
            &slot_bytes,
        ])
        .to_bytes()
    }

    /// Add a merchant leaf to the Merkle tree by appending.
    /// Called by the platform authority.
    pub async fn add_merchant(&self, payer: &Keypair, leaf: &MerchantLeaf) -> Result<Signature> {
        let leaf_hash = Self::compute_leaf_hash(leaf);
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let program_id = spl_account_compression::id();
        let noop_program = spl_noop::id();

        // Build Anchor instruction: discriminator + borsh-serialized params
        // append(leaf: [u8; 32]) → 8 + 32 = 40 bytes
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&anchor_sighash("append"));
        data.extend_from_slice(&leaf_hash);

        let accounts = vec![
            AccountMeta::new(self.tree_address, false),
            AccountMeta::new_readonly(self.tree_authority, true),
            AccountMeta::new_readonly(noop_program, false),
        ];

        let ix = Instruction {
            program_id,
            accounts,
            data,
        };

        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    /// Update a merchant leaf in the Merkle tree (e.g., key rotation).
    /// Requires the old leaf hash, new leaf, leaf index, a Merkle proof, and an IndexerClient for root retrieval.
    pub async fn update_merchant(
        &self,
        payer: &Keypair,
        old_leaf: &MerchantLeaf,
        new_leaf: &MerchantLeaf,
        leaf_index: u32,
        proof: &[[u8; 32]],
        indexer: &IndexerClient,
    ) -> Result<Signature> {
        let old_hash = Self::compute_leaf_hash(old_leaf);
        let new_hash = Self::compute_leaf_hash(new_leaf);
        let root = indexer.get_tree_root(&self.tree_address).await?;

        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| SolanaError::RpcError(e.to_string()))?;

        let program_id = spl_account_compression::id();
        let noop_program = spl_noop::id();

        // replace_leaf(root: [u8;32], previous_leaf: [u8;32], new_leaf: [u8;32], index: u32)
        // → 8 + 32 + 32 + 32 + 4 = 108 bytes
        let mut data = Vec::with_capacity(108);
        data.extend_from_slice(&anchor_sighash("replace_leaf"));
        data.extend_from_slice(&root);
        data.extend_from_slice(&old_hash);
        data.extend_from_slice(&new_hash);
        data.extend_from_slice(&leaf_index.to_le_bytes());

        let mut accounts = vec![
            AccountMeta::new(self.tree_address, false),
            AccountMeta::new_readonly(self.tree_authority, true),
            AccountMeta::new_readonly(noop_program, false),
        ];
        // Proof nodes as remaining accounts
        for node in proof {
            let pubkey = Pubkey::new_from_array(*node);
            accounts.push(AccountMeta::new_readonly(pubkey, false));
        }

        let ix = Instruction {
            program_id,
            accounts,
            data,
        };

        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        let sig = self
            .rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| SolanaError::TransactionFailed(e.to_string()))?;

        Ok(sig)
    }

    /// Fetch the current root hash via the IndexerClient (DAS API).
    /// This is the recommended way to get the root — on-chain parsing is fragile.
    pub async fn get_tree_root_via_indexer(
        &self,
        indexer: &IndexerClient,
    ) -> Result<[u8; 32]> {
        indexer.get_tree_root(&self.tree_address).await
    }

    /// Verify a Merkle proof locally (off-chain fast filter).
    /// Walks the proof from leaf to root, hashing at each level.
    pub fn verify_proof_locally(
        &self,
        leaf_hash: &[u8; 32],
        proof: &[[u8; 32]],
        root: &[u8; 32],
    ) -> bool {
        let mut current = *leaf_hash;
        for sibling in proof {
            let (left, right) = if current < *sibling {
                (current, *sibling)
            } else {
                (*sibling, current)
            };
            current = hashv(&[&left, &right]).to_bytes();
        }
        current == *root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_leaf_hash_deterministic() {
        let leaf = MerchantLeaf {
            merchant_did_hash: [1u8; 32],
            active_pubkey: Pubkey::new_unique(),
            platform_vc_hash: [2u8; 32],
            status: crate::types::MERCHANT_STATUS_ACTIVE,
            slot_updated: 100,
        };
        let h1 = CompressionService::compute_leaf_hash(&leaf);
        let h2 = CompressionService::compute_leaf_hash(&leaf);
        assert_eq!(h1, h2, "Leaf hash should be deterministic");
    }

    #[test]
    fn test_compute_leaf_hash_different_inputs() {
        let leaf1 = MerchantLeaf {
            merchant_did_hash: [1u8; 32],
            active_pubkey: Pubkey::new_unique(),
            platform_vc_hash: [2u8; 32],
            status: crate::types::MERCHANT_STATUS_ACTIVE,
            slot_updated: 100,
        };
        let leaf2 = MerchantLeaf {
            merchant_did_hash: [3u8; 32],
            active_pubkey: Pubkey::new_unique(),
            platform_vc_hash: [4u8; 32],
            status: crate::types::MERCHANT_STATUS_REVOKED,
            slot_updated: 200,
        };
        let h1 = CompressionService::compute_leaf_hash(&leaf1);
        let h2 = CompressionService::compute_leaf_hash(&leaf2);
        assert_ne!(h1, h2, "Different leaves should produce different hashes");
    }

    #[test]
    fn test_compute_leaf_hash_status_matters() {
        let mut leaf1 = MerchantLeaf {
            merchant_did_hash: [1u8; 32],
            active_pubkey: Pubkey::new_unique(),
            platform_vc_hash: [2u8; 32],
            status: crate::types::MERCHANT_STATUS_ACTIVE,
            slot_updated: 100,
        };
        let leaf2 = MerchantLeaf {
            merchant_did_hash: leaf1.merchant_did_hash,
            active_pubkey: leaf1.active_pubkey,
            platform_vc_hash: leaf1.platform_vc_hash,
            status: crate::types::MERCHANT_STATUS_SUSPENDED,
            slot_updated: leaf1.slot_updated,
        };
        let h1 = CompressionService::compute_leaf_hash(&leaf1);
        let h2 = CompressionService::compute_leaf_hash(&leaf2);
        assert_ne!(h1, h2, "Status change should produce different hash");
    }

    #[test]
    fn test_verify_proof_locally_single_leaf() {
        let leaf_hash = [5u8; 32];
        let proof: Vec<[u8; 32]> = vec![];
        let root = leaf_hash;
        let service = CompressionService::new(
            "https://api.devnet.solana.com",
            "11111111111111111111111111111111",
            "11111111111111111111111111111111",
        )
        .unwrap();
        assert!(
            service.verify_proof_locally(&leaf_hash, &proof, &root),
            "Single leaf tree: root should equal leaf hash"
        );
    }

    #[test]
    fn test_verify_proof_locally_two_leaves() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let (left, right) = if leaf1 < leaf2 {
            (leaf1, leaf2)
        } else {
            (leaf2, leaf1)
        };
        let root = hashv(&[&left, &right]).to_bytes();

        let service = CompressionService::new(
            "https://api.devnet.solana.com",
            "11111111111111111111111111111111",
            "11111111111111111111111111111111",
        )
        .unwrap();

        assert!(
            service.verify_proof_locally(&leaf1, &[leaf2], &root),
            "Proof for leaf1 should verify"
        );
        assert!(
            service.verify_proof_locally(&leaf2, &[leaf1], &root),
            "Proof for leaf2 should verify"
        );
    }

    #[test]
    fn test_verify_proof_locally_fails_wrong_root() {
        let leaf = [1u8; 32];
        let sibling = [2u8; 32];
        let wrong_root = [99u8; 32];
        let service = CompressionService::new(
            "https://api.devnet.solana.com",
            "11111111111111111111111111111111",
            "11111111111111111111111111111111",
        )
        .unwrap();
        assert!(
            !service.verify_proof_locally(&leaf, &[sibling], &wrong_root),
            "Should fail with wrong root"
        );
    }

    #[test]
    fn test_anchor_sighash_deterministic() {
        let h1 = anchor_sighash("append");
        let h2 = anchor_sighash("append");
        assert_eq!(h1, h2);
        let h3 = anchor_sighash("replace_leaf");
        assert_ne!(
            h1, h3,
            "Different instruction names should have different sighashes"
        );
    }
}
