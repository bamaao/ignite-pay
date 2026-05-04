//! Bridge layer between ignite-pay-core and ignite-pay-solana.
//! Provides DID-related on-chain operations for merchant verification
//! via ZK compressed accounts.

use anyhow::Result;
use ignite_pay_solana::compression::DidService;
use ignite_pay_solana::solana_sdk::pubkey::Pubkey;
use serde_json::json;

use crate::identity::extract_pubkey_from_did;

/// Bridge for Solana DID verification operations via ZK compressed accounts.
pub struct SolanaDidBridge {
    did_service: DidService,
    photon_url: String,
    address_tree: Pubkey,
}

impl std::fmt::Debug for SolanaDidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaDidBridge")
            .field("photon_url", &self.photon_url)
            .field("address_tree", &self.address_tree)
            .finish()
    }
}

impl SolanaDidBridge {
    /// Create a new SolanaDidBridge.
    pub fn new(rpc_url: &str, did_program_id: &str, photon_url: &str, address_tree: &str) -> Result<Self> {
        let did_service = DidService::new(rpc_url, did_program_id)?;
        let address_tree = address_tree.parse::<Pubkey>()
            .map_err(|e| anyhow::anyhow!("Invalid address_tree pubkey: {}", e))?;
        Ok(Self {
            did_service,
            photon_url: photon_url.to_string(),
            address_tree,
        })
    }

    /// Get a reference to the underlying DidService.
    pub fn did_service(&self) -> &DidService {
        &self.did_service
    }

    /// Verify a merchant DID exists on-chain as a ZK compressed account.
    ///
    /// Steps:
    /// 1. Extract Ed25519 public key from `did:ignite:z...`
    /// 2. Derive the compressed PDA address via `DidService::derive_compressed_address`
    /// 3. Query Photon API `getCompressedAccount` to check existence
    pub async fn quick_verify(&self, merchant_did: &str) -> Result<bool> {
        // 1. Extract public key bytes from DID
        let pk_bytes = extract_pubkey_from_did(merchant_did)
            .ok_or_else(|| anyhow::anyhow!("Invalid DID format: {}", merchant_did))?;

        let original_pk = Pubkey::new_from_array(pk_bytes);

        // 2. Derive compressed address
        let (address, _) = self.did_service.derive_compressed_address(&original_pk, &self.address_tree);
        let address_b58 = bs58::encode(address).into_string();

        // 3. Query Photon API
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/getCompressedAccount", self.photon_url))
            .json(&json!({ "address": address_b58 }))
            .send()
            .await?;

        if !resp.status().is_success() {
            // Non-success HTTP status means the account doesn't exist or API error
            return Ok(false);
        }

        let body: serde_json::Value = resp.json().await?;
        // Photon returns { data: { ... } } when account exists, or null/empty when not
        Ok(body.get("data").is_some())
    }
}
